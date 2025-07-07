use std::sync::{Arc, Mutex};

fn main() {
    let a = Arc::new(Mutex::new(999));
    println!("{}", a.lock().unwrap());
    *a.lock().unwrap() = 1;
    println!("{}", a.lock().unwrap());
    *a.lock().unwrap() = 2;
    println!("{}", a.lock().unwrap());
    *a.lock().unwrap() = 3;
    println!("{}", a.lock().unwrap());
    *a.lock().unwrap() = 4;
    println!("{}", a.lock().unwrap());
    *a.lock().unwrap() = 5;
    println!("{}", a.lock().unwrap());

    /* we got a variable here, so it will lock until variable drops */
    let mut lock = a.lock().unwrap();
    *lock = 1;
    println!("{}", a.lock().unwrap());
}
