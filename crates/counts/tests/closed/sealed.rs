//! Another crate implementing `Closed` for its own type: refused, since the trait is sealed.

struct Free(String);

impl clauseo_counts::schema::Closed for Free {}

fn main() {
    drop(Free(String::new()).0);
}
