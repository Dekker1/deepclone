use deepclone::DeepClone;

#[derive(DeepClone)]
struct Holder {
    #[deepclone(shallow)]
    value: u32,
}

fn main() {}
