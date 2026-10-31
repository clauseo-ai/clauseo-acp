use clauseo_protocol::Cursor;

use super::PatchWindow;

fn tail(window: &PatchWindow, after: u64) -> Option<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let held = window
        .tail(Cursor::new(after).unwrap(), |json| {
            out.push(json.to_vec());
            Ok::<(), ()>(())
        })
        .unwrap();
    held.then_some(out)
}

fn patch(n: u8, len: usize) -> Vec<u8> {
    vec![b'a' + n % 26; len]
}

#[test]
fn drops_oldest_past_the_cap() {
    let mut window = PatchWindow::new(100);
    for n in 0..10 {
        window.push(&patch(n, 30));
        assert!(window.bytes_held() <= 100);
    }
    // Cursors 8, 9 and 10 fit (90 bytes); 7 and before dropped.
    assert_eq!(window.bytes_held(), 90);
    assert_eq!(
        tail(&window, 7).unwrap(),
        vec![patch(7, 30), patch(8, 30), patch(9, 30)]
    );
    assert_eq!(tail(&window, 9).unwrap(), vec![patch(9, 30)]);
    assert_eq!(tail(&window, 10).unwrap(), Vec::<Vec<u8>>::new());
    assert!(tail(&window, 6).is_none(), "fell out");
    assert!(tail(&window, 11).is_none(), "past the end");
}

#[test]
fn patch_over_the_cap_empties_the_window() {
    let mut window = PatchWindow::new(100);
    window.push(&patch(0, 40));
    window.push(&patch(1, 101));
    assert_eq!(window.bytes_held(), 0);
    assert_eq!(tail(&window, 2).unwrap(), Vec::<Vec<u8>>::new());
    assert!(tail(&window, 1).is_none());
    window.push(&patch(2, 10));
    assert_eq!(tail(&window, 2).unwrap(), vec![patch(2, 10)]);
}

#[test]
fn tail_reads_patches_across_the_ring_wrap() {
    let mut window = PatchWindow::new(64);
    // Uneven lengths move the ring's head around its buffer, so some patches straddle the end.
    let mut pushed = Vec::new();
    for n in 0..200u8 {
        let p = patch(n, 5 + usize::from(n % 13));
        window.push(&p);
        pushed.push(p);
        let mut expected = Vec::new();
        let mut sum = 0;
        for p in pushed.iter().rev() {
            if sum + p.len() > 64 {
                break;
            }
            sum += p.len();
            expected.push(p.clone());
        }
        expected.reverse();
        let first = u64::from(n) + 1 - expected.len() as u64;
        assert_eq!(tail(&window, first).unwrap(), expected, "after patch {n}");
        assert_eq!(window.bytes_held(), sum);
    }
}
