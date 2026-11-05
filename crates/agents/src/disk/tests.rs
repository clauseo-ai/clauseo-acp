use super::*;

#[test]
fn the_temp_folder_has_free_space() {
    let free = free_bytes(&std::env::temp_dir()).unwrap();
    assert!(free > 0);
}

#[test]
fn a_folder_not_made_yet_counts_its_nearest_existing_ancestor() {
    let missing = std::env::temp_dir()
        .join("clauseo-agents-not-made")
        .join("data");
    assert!(free_bytes(&missing).unwrap() > 0);
}
