use deepclone::DeepClone;

#[derive(Clone, DeepClone)]
#[deepclone(clone)]
struct Holder {
    #[deepclone(default)]
    value: u32,
}

fn main() {}
