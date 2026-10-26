#[global_allocator]
static ALLOC: kiroha::alloc::Counting = kiroha::alloc::Counting;

fn main() {
    // Run on a thread with a large stack: deeply nested source/JSON values are
    // processed recursively by the frontend and serializer.
    let handle = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(kiroha::cli::main)
        .expect("spawn main thread");
    let code = handle.join().unwrap_or(70);
    std::process::exit(code);
}
