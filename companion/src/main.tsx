import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { Approve } from "./Approve";
import { LockGate } from "./LockScreen";
import "./styles.css";

// the extension opens `index.html#/approve/<id>` in a small window when a website asks for the account or a sign-in
const approve = /^#\/approve\/([\w-]+)$/.exec(location.hash);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <LockGate>{approve ? <Approve id={approve[1]} /> : <App />}</LockGate>
  </StrictMode>,
);
