fn main() {
    println!("Hello, world!");


    let paths = std::fs::read_dir("./").unwrap();

    for path in paths {
        println!("Name: {:?}", path.unwrap().path())
    }
}
