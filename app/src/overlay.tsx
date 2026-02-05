import React from "react";
import ReactDOM from "react-dom/client";
import OverlayApp from "./OverlayApp";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <OverlayApp />
  </React.StrictMode>,
);
