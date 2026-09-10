fn main() {
    // .slint 源码统一放在工作区根 ui/ 目录
    slint_build::compile("../../ui/gateway.slint").expect("Slint 编译失败");
}
