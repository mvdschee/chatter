fn main() {
	println!("cargo:rustc-link-arg=-Tdefmt.x");
	// linkall.x must be the last linker script
	println!("cargo:rustc-link-arg=-Tlinkall.x");
}
