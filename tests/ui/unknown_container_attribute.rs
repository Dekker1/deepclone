use deepclone::DeepClone;

#[derive(DeepClone)]
#[deepclone(transparent)]
struct Holder {
    value: u32,
}

fn main() {}
