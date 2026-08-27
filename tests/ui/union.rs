use deepclone::DeepClone;

#[derive(DeepClone)]
union Choice {
    a: u32,
    b: f32,
}

fn main() {}
