package com.termux.app;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import com.termux.shared.logger.Logger;

import java.util.ArrayList;
import java.util.List;

/**
 * Thread-safe lifecycle state coordinator and callback registry for Termux bootstrap installation.
 * Decouples Activity recreation and lifecycle events from background installation progress.
 */
public final class BootstrapState {

    private static final String LOG_TAG = "BootstrapState";

    public enum Stage {
        IDLE("Idle"),
        PRECONDITIONS("Checking Preconditions"),
        CLEANUP_STAGING("Cleaning Staging Directory"),
        CLEANUP_PREFIX("Cleaning Prefix Directory"),
        CREATE_STAGING("Creating Staging Directory"),
        CREATE_PREFIX("Creating Prefix Directory"),
        LOAD_ZIP("Loading Bootstrap Archive"),
        EXTRACT_ZIP("Extracting Bootstrap Archive"),
        VERIFY_STAGING("Verifying Staging Directory"),
        PROMOTE_PREFIX("Promoting Staging to Prefix"),
        SECOND_STAGE("Executing Second Stage Initialization"),
        VERIFY_FINAL_PREFIX("Verifying Final Prefix"),
        WRITE_ENVIRONMENT("Writing Environment Configuration"),
        READY("Ready"),
        FAILED("Failed");

        private final String displayName;

        Stage(String displayName) {
            this.displayName = displayName;
        }

        public String getDisplayName() {
            return displayName;
        }
    }

    private static volatile Stage sCurrentStage = Stage.IDLE;
    private static final List<Runnable> sPendingCallbacks = new ArrayList<>();
    private static String sFailureReason = null;
    private static Stage sFailedStage = null;

    private BootstrapState() {}

    /**
     * Updates the current bootstrap installation stage.
     */
    public static synchronized void setStage(@NonNull Stage stage) {
        Logger.logInfo(LOG_TAG, "Transition: " + sCurrentStage + " -> " + stage);
        sCurrentStage = stage;
        if (stage == Stage.READY) {
            sFailureReason = null;
            sFailedStage = null;
        }
    }

    @NonNull
    public static Stage getStage() {
        return sCurrentStage;
    }

    public static boolean isRunning() {
        Stage s = sCurrentStage;
        return s != Stage.IDLE && s != Stage.READY && s != Stage.FAILED;
    }

    public static boolean isReady() {
        return sCurrentStage == Stage.READY;
    }

    public static boolean isFailed() {
        return sCurrentStage == Stage.FAILED;
    }

    @Nullable
    public static String getFailureReason() {
        return sFailureReason;
    }

    @Nullable
    public static Stage getFailedStage() {
        return sFailedStage;
    }

    /**
     * Registers a callback to be executed when bootstrap installation completes successfully.
     * If bootstrap is already READY, the callback is executed immediately.
     */
    public static synchronized void addCallback(@NonNull Runnable callback) {
        if (isReady()) {
            Logger.logInfo(LOG_TAG, "Bootstrap already ready; invoking callback immediately.");
            callback.run();
            return;
        }
        Logger.logInfo(LOG_TAG, "Registering pending completion callback (total=" + (sPendingCallbacks.size() + 1) + ")");
        sPendingCallbacks.add(callback);
    }

    /**
     * Dispatches success to all registered callbacks and transitions stage to READY.
     */
    public static synchronized void dispatchSuccess() {
        setStage(Stage.READY);
        List<Runnable> callbacks = new ArrayList<>(sPendingCallbacks);
        sPendingCallbacks.clear();
        Logger.logInfo(LOG_TAG, "Dispatching bootstrap completion to " + callbacks.size() + " listeners");
        for (Runnable r : callbacks) {
            try {
                r.run();
            } catch (Throwable t) {
                Logger.logError(LOG_TAG, "Error running bootstrap completion callback: " + t.getMessage());
            }
        }
    }

    /**
     * Records installation failure and clears pending callbacks.
     */
    public static synchronized void dispatchFailure(@NonNull Stage failingStage, @Nullable String reason) {
        sFailedStage = failingStage;
        sFailureReason = reason != null ? reason : "Unknown error at stage " + failingStage.getDisplayName();
        setStage(Stage.FAILED);
        Logger.logError(LOG_TAG, "Bootstrap installation failed at stage [" + failingStage + "]: " + sFailureReason);
        sPendingCallbacks.clear();
    }

    /**
     * Resets state back to IDLE (for retries).
     */
    public static synchronized void reset() {
        Logger.logInfo(LOG_TAG, "Resetting bootstrap state to IDLE");
        sCurrentStage = Stage.IDLE;
        sPendingCallbacks.clear();
        sFailureReason = null;
        sFailedStage = null;
    }
}
