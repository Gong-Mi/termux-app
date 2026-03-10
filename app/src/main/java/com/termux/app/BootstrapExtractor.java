package com.termux.app;

import com.termux.shared.logger.Logger;

/**
 * Bootstrap Extractor using Rust implementation
 * Uses Rust to extract bootstrap zip bytes to target directory
 */
public class BootstrapExtractor {
    
    private static final String LOG_TAG = "BootstrapExtractor";
    
    // Load the Rust library from terminal-emulator module
    static {
        System.loadLibrary("termux_rust");
    }
    
    /**
     * Extract bootstrap zip from bytes to target directory
     * 
     * @param zipBytes The bootstrap zip file as byte array
     * @param targetDir The target directory path to extract to
     * @return Number of files extracted, or negative error code:
     *         -1: JNI error
     *         -2: Path error
     *         -3: Byte array conversion error
     *         -4: Extract error
     */
    public static native long extractFromBytes(byte[] zipBytes, String targetDir);
    
    /**
     * Extract bootstrap zip using Rust implementation
     * 
     * @param zipBytes The bootstrap zip file as byte array
     * @param targetDir The target directory path to extract to
     * @return true if successful, false otherwise
     */
    public static boolean extractBootstrap(byte[] zipBytes, String targetDir) {
        Logger.logInfo(LOG_TAG, "Starting Rust bootstrap extraction to: " + targetDir);
        
        long result = extractFromBytes(zipBytes, targetDir);
        
        if (result < 0) {
            Logger.logError(LOG_TAG, "Bootstrap extraction failed with error code: " + result);
            return false;
        }
        
        Logger.logInfo(LOG_TAG, "Bootstrap extraction completed successfully. Extracted " + result + " files.");
        return true;
    }
}
