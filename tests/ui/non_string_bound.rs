use deepclone::DeepClone;

#[derive(DeepClone)]
#[deepclone(bound = 7)]
struct Holder<T> {
    value: T,
}

fn main() {}
