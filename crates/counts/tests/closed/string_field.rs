//! A payload struct with a `String` field: refused, since `String` is not `Closed`.

pub struct Opened {
    pub launch: clauseo_counts::schema::Launch,
    pub matter: String,
}

clauseo_counts::closed_fields!(Opened { launch, matter });

fn main() {}
