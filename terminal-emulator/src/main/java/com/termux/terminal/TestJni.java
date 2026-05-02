package com.termux.terminal;

public class TestJni {
    public static void main(String[] args) {
        System.loadLibrary("termux_rust");
        System.out.println("Loaded library");
        try {
            int w = WcWidth.widthRust(65); // 'A'
            System.out.println("Width of A: " + w);
        } catch (Throwable e) {
            e.printStackTrace();
        }
    }
}
