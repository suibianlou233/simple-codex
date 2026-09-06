import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { recordFrontendDiagnostic } from "./bridge/tauriBridge";
import { applyInitialTheme } from "./theme";
import "./styles.css";
import "./design/workbench.css";

applyInitialTheme();

function diagnosticMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "unknown React error";
}

window.addEventListener("error", (event) => {
  recordFrontendDiagnostic("error", "window_error", event.message, {
    filename: event.filename,
    line: event.lineno,
    column: event.colno,
  });
});

window.addEventListener("unhandledrejection", (event) => {
  const message = event.reason instanceof Error
    ? event.reason.message
    : typeof event.reason === "string"
      ? event.reason
      : "unhandled promise rejection";
  recordFrontendDiagnostic("error", "unhandled_rejection", message);
});

const root = document.getElementById("root");

if (!root) {
  throw new Error("Desktop root element was not found.");
}

createRoot(root, {
  onUncaughtError(error) {
    recordFrontendDiagnostic("error", "react_uncaught_error", diagnosticMessage(error));
  },
  onCaughtError(error) {
    recordFrontendDiagnostic("error", "react_caught_error", diagnosticMessage(error));
  },
  onRecoverableError(error) {
    recordFrontendDiagnostic("warn", "react_recoverable_error", diagnosticMessage(error));
  },
}).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

recordFrontendDiagnostic("info", "application_mounted");
