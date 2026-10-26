import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import {DisplayTimeProvider} from "./shared/display-time";
import "./index.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <DisplayTimeProvider><App /></DisplayTimeProvider>
  </StrictMode>,
);
