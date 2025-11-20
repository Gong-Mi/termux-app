/*
 * Diagnosis of GLES (OpenGL ES) Issue in com.termux:
 *
 * Problem:
 * Logcat analysis (from termux_diagnostics.log) revealed an AndroidRuntime crash
 * originating from `com.termux.view.TerminalRendererGLES.java`.
 * Specifically, the crashes were observed in:
 * - `TerminalRendererGLES.generateMesh(TerminalRendererGLES.java:172)`
 * - `TerminalRendererGLES.onDrawFrame(TerminalRendererGLES.java:254)`
 *
 * Root Cause Hypothesis:
 * The crash likely stems from issues with OpenGL ES buffers used for rendering
 * terminal content. Potential causes include:
 * 1. Incorrect sizing or reallocation of vertex/color/texture buffers (`mVertexBuffer`, `mColorBuffer`, `mTextureBuffer`).
 * 2. Out-of-memory errors during buffer allocation.
 * 3. Passing invalid data or states to GLES functions.
 *
 * Debugging Steps Taken:
 * 1. Added extensive logging within `generateMesh()` to track:
 *    - `columns`, `rows`, `numCharacters`, `numVertices` calculations.
 *    - Capacity of `mVertexBuffer`, `mTextureBuffer`, `mColorBuffer` after allocation/reallocation.
 *    - Position, limit, and capacity of `mColorBuffer` before `put()` operations.
 * 2. Implemented `checkGlError()` calls after each significant GLES operation
 *    in `onDrawFrame()` and `onSurfaceCreated()` to catch specific GLES API errors.
 * 3. Updated the logging TAG to use `TerminalRendererGLES.class.getSimpleName()`
 *    for clearer log identification.
 *
 * Next Steps:
 * Recompile the application with the added logging, reproduce the GLES crash,
 * and then re-run this `diagnostic_tool` to capture the enriched log data for
 * further analysis.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/system_properties.h>
#include <time.h>

// A helper function to execute a command and write its output to a file
void run_command_and_write_output(FILE *fp, const char* command) {
    char buffer[1024];
    FILE *pipe;

    pipe = popen(command, "r");
    if (!pipe) {
        fprintf(fp, "Failed to run command: %s\n", command);
        return;
    }

    while (fgets(buffer, sizeof(buffer), pipe) != NULL) {
        fprintf(fp, "%s", buffer);
    }

    pclose(pipe);
}

// A helper function to get a system property and write it to a file
void get_property_and_write_output(FILE *fp, const char* property_name, const char* display_name) {
    char value[PROP_VALUE_MAX];
    if (__system_property_get(property_name, value) > 0) {
        fprintf(fp, "%s: %s\n", display_name, value);
    } else {
        fprintf(fp, "%s: Not Found\n", display_name);
    }
}

int main() {
    const char* filename = "termux_diagnostics.log";
    FILE *fp = fopen(filename, "w");

    if (!fp) {
        perror("Failed to open log file");
        return 1;
    }

    printf("Starting diagnostic collection...\n");
    printf("Output will be saved to: %s\n", filename);

    // --- Collect System Properties ---
    fprintf(fp, "--- System Properties ---\n");
    get_property_and_write_output(fp, "ro.build.version.sdk", "Android SDK Version");
    get_property_and_write_output(fp, "ro.build.version.release", "Android Release Version");
    get_property_and_write_output(fp, "ro.product.model", "Device Model");
    get_property_and_write_output(fp, "ro.product.manufacturer", "Device Manufacturer");
    get_property_and_write_output(fp, "ro.product.cpu.abi", "CPU ABI");
    fprintf(fp, "\n");

    // --- Collect Graphics Properties ---
    fprintf(fp, "--- Graphics Properties ---\n");
    // This property often holds the GLES version string
    get_property_and_write_output(fp, "ro.opengles.version", "GLES Version (from getprop)");
    fprintf(fp, "\n--- SurfaceFlinger GLES Info ---\n");
    run_command_and_write_output(fp, "/system/bin/dumpsys SurfaceFlinger | grep 'GLES'");
    fprintf(fp, "\n");

    // --- Collect Termux Logs ---
    fprintf(fp, "--- Termux Logcat ---\n");
    run_command_and_write_output(fp, "logcat -d | grep 'com.termux'");

    fclose(fp);

    printf("\nDiagnostic collection complete.\n");
    printf("Data has been saved to '%s'.\n", filename);

    return 0;
}
