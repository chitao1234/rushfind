use std::ffi::OsString;

pub fn run<I>(args: I) -> i32
where
    I: IntoIterator<Item = OsString>,
{
    let _ = args.into_iter().collect::<Vec<_>>();
    0
}
