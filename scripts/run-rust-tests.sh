#!/usr/bin/env bash
# Run the Rust terminal test suites by explicit, reviewable tier.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUST_DIR="$ROOT_DIR/terminal-emulator/src/main/rust"

run_tests() {
    local tier="$1"
    shift
    printf '\n== Rust test tier: %s ==\n' "$tier"
    local -a targets=()
    local test_name
    for test_name in "$@"; do
        targets+=(--test "$test_name")
    done
    # Resolve/build the tier once, retaining explicit target selection. Cargo
    # executes test binaries serially; a failing binary must not hide later ones.
    cargo test --locked --no-fail-fast "${targets[@]}" -- --test-threads=1
}

cd "$RUST_DIR"

tier=${1:-core}
case "$tier" in
    core)
        run_tests core \
            consistency \
            extended_features \
            check_width \
            wcwidth_boundaries \
            utf8_streaming \
            fix_verification \
            key_event_handling \
            osc_escape_test \
            selection_test \
            selection_pipeline_test \
            vt_compatibility
        ;;
    regressions)
        run_tests regressions \
            reflow_stress \
            resize_history_bug \
            resize_regression_final \
            resize_zoom_simulation \
            screen_expand_content_loss \
            test_active_transcript_rows \
            test_expand_content_below \
            test_wraparound_active_transcript
        ;;
    terminal)
        run_tests terminal \
            block_coverage_test \
            block_element_render_test \
            box_drawing_geometry_verify \
            box_drawing_layout_stress_test \
            box_drawing_overlap_verify \
            clear_screen_test \
            crlf_test \
            gemini_compat_test \
            gemini_state_soak_test \
            inverse_video_test \
            lnm_problems \
            lnm_test \
            lnm_verify \
            mismatch_verification \
            sixel_color_test \
            truecolor_render_test
        ;;
    lifecycle)
        run_tests lifecycle \
            engine_handle_lifecycle \
            engine_handle_integration \
            engine_delivery_claim \
            pty_io_runtime \
            pty_context_integration \
            process_owner \
            session_process_lifecycle \
            session_completion_observation \
            concurrency_lock_safety \
            flat_buffer_test \
            jni_boundary_safety \
            multi_session_concurrency \
            phantom_thread_test \
            render_deadlock_test \
            session_coordinator_integration \
            session_coordinator_test \
            surface_crash_repro \
            test_jni_null
        ;;
    render)
        run_tests render \
            font_calibration \
            logo_render_chain_test \
            skia_basic_test \
            skia_render_test \
            vulkan_cache_performance_proof \
            vulkan_lifecycle_stress \
            vulkan_path_cache_test \
            vulkan_physical_cache_test \
            vulkan_render_benchmark
        ;;
    perf)
        run_tests perf \
            extreme_content_stress \
            performance \
            resize_benchmark \
            vte_parser_benchmark \
            vte_performance_benchmark \
            vte_sve_consistency
        ;;
    all)
        # Keep the complete integration-test manifest explicit. A new test
        # file must be added here (and to its narrower tier) before it can be
        # considered part of the acceptance surface.
        ALL_TESTS=(
            analyze_exceptions block_coverage_test block_element_render_test
            box_drawing_geometry_verify box_drawing_layout_stress_test
            box_drawing_overlap_verify check_width clear_screen_test
            concurrency_lock_safety consistency content_overflow_test crlf_test
            engine_handle_lifecycle engine_handle_integration engine_delivery_claim
            pty_io_runtime pty_context_integration process_owner session_process_lifecycle
            session_completion_observation
            extended_features extreme_content_stress fix_verification
            flat_buffer_test font_calibration gemini_compat_test
            gemini_state_soak_test inverse_video_test jni_boundary_safety
            key_event_handling lnm_problems lnm_test lnm_verify logger_smoke_test
            logo_render_chain_test mismatch_verification multi_session_concurrency
            osc_escape_test performance phantom_thread_test reflow_600_lines
            reflow_stress reflow_trap_repro render_deadlock_test resize_benchmark
            resize_column_change resize_history_bug resize_history_debug
            resize_regression_final resize_zoom_simulation
            screen_expand_content_loss selection_color_test selection_pipeline_test
            selection_test session_coordinator_integration session_coordinator_test
            sixel_color_test skia_basic_test skia_render_test surface_crash_repro
            test_active_transcript_rows test_clipboard_render test_data_integrity
            test_expand_content_below test_jni_null test_row_bounds_checking
            test_selection_custom test_wcwidth_integration
            test_wraparound_active_transcript truecolor_render_test
            vt_compatibility vte_parser_benchmark vte_performance_benchmark
            vte_sve_consistency vulkan_cache_performance_proof
            vulkan_lifecycle_stress vulkan_path_cache_test
            vulkan_physical_cache_test vulkan_render_benchmark wcwidth_boundaries utf8_streaming
        )
        run_tests all "${ALL_TESTS[@]}"
        ;;
    *)
        printf 'Usage: %s {core|regressions|terminal|lifecycle|render|perf|all}\n' "$0" >&2
        exit 2
        ;;
esac
