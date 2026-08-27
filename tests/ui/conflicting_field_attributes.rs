use deepclone::DeepClone;

#[derive(DeepClone)]
struct Holder {
    #[deepclone(clone)]
    #[deepclone(default)]
    value: u32,
}

fn main() {}
