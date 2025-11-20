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
