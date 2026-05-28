fn main() {
  #[cfg(windows)]
  {
    let mut res = winres::WindowsResource::new();
    res.set_icon("src/assets/bolt.ico");
    res.compile().expect("Failed to compile Windows resources");
  }
}

