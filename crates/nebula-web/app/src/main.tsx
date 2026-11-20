import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { Deck } from "./DeckPage";
import { isDeck } from "./deck";
import { watchTheme } from "./theme";
import "./tokens.css";
import "./app.css";

watchTheme();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {isDeck(location.pathname) ? <Deck /> : <App />}
  </StrictMode>,
);
