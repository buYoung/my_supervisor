import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import {DisplayTimeProvider} from "./shared/display-time";
import "./index.css";

async function mount() {
  if (import.meta.env.VITE_DESKTOP_E2E) {
    await import("./testing/native-harness");
  }
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <DisplayTimeProvider><App /></DisplayTimeProvider>
    </StrictMode>,
  );
}
void mount();
