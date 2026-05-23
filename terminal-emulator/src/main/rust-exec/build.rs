fn main() {
    cc::Build::new()
        .file("src/get_regs.c")
        .compile("termux_exec_get_regs");
}
