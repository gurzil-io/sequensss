//! Windows: icon and metadata embedded in the .exe.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/sequensss.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/sequensss.ico");
        res.compile().expect("Windows resources (icon)");
    }
}
