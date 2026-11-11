extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{FileType, VFS};
use crate::mm::pmm::PMM;
use crate::{print, println};

pub fn run_all_tests() {
    println!("\n=======================================================");
    println!("             CosmosKernel Verification Suite           ");
    println!("=======================================================");

    test_pmm();
    test_heap();
    test_vfs();
    test_pipes();
    test_multitasking();

    println!("=======================================================");
    println!("      All Kernel Self-Tests PASSED Successfully!       ");
    println!("=======================================================\n");
}

fn test_pmm() {
    print!("  [TEST] Physical Frame Allocator ... ");
    let mut pmm = PMM.lock();
    let frame1 = pmm.alloc_frame().expect("PMM frame 1 alloc failed");
    let frame2 = pmm.alloc_frame().expect("PMM frame 2 alloc failed");
    assert!(frame1 != frame2, "PMM allocated identical frames");
    pmm.free_frame(frame1);
    pmm.free_frame(frame2);
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_heap() {
    print!("  [TEST] Dynamic Heap & Alloc Crate ... ");
    let b = Box::new(42u64);
    assert_eq!(*b, 42);

    let mut v = Vec::new();
    for i in 0..1000 {
        v.push(i);
    }
    assert_eq!(v.len(), 1000);
    assert_eq!(v[999], 999);

    let s = String::from("CosmosKernel Heap Allocation Works!");
    assert_eq!(s.len(), 35);
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_vfs() {
    print!("  [TEST] Virtual File System & Mounts ... ");
    let vfs = VFS.lock();

    // Test CosmicOS /System/Library/CoreServices/SystemVersion.plist
    let file = vfs.open("/System/Library/CoreServices/SystemVersion.plist", 0).expect("Failed to open SystemVersion.plist");
    let mut buf = [0u8; 128];
    let n = file.read(&mut buf).expect("Failed to read SystemVersion.plist");
    assert!(n > 0);

    // Test writing and reading temporary file in /tmp
    let tmp = vfs.resolve_path("/tmp").expect("Failed to resolve /tmp");
    let test_file_node = tmp.lock().create("test.txt", FileType::Regular).expect("Failed to create /tmp/test.txt");
    test_file_node.lock().write(0, b"Hello Cosmos!").expect("Failed to write to /tmp/test.txt");

    let mut read_buf = [0u8; 13];
    test_file_node.lock().read(0, &mut read_buf).expect("Failed to read from /tmp/test.txt");
    assert_eq!(&read_buf, b"Hello Cosmos!");

    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_pipes() {
    print!("  [TEST] Anonymous IPC Pipes ... ");
    let (reader, writer) = crate::ipc::create_pipe();
    writer.lock().write(0, b"IPC Message 123").expect("Pipe write failed");

    let mut buf = [0u8; 15];
    let n = reader.lock().read(0, &mut buf).expect("Pipe read failed");
    assert_eq!(n, 15);
    assert_eq!(&buf, b"IPC Message 123");
    println!("\x1b[32m[PASS]\x1b[0m");
}

fn test_multitasking() {
    print!("  [TEST] Task Context Switch & Yield ... ");
    // Yielding should return back to current thread smoothly
    crate::task::scheduler::yield_now();
    println!("\x1b[32m[PASS]\x1b[0m");
}
