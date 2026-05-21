import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import SetupApp from "./setup/SetupApp";
import "./index.css";
import { loadRuntimeConfig } from "./lib/runtime-config";

const root = createRoot(document.getElementById("root")!);

function renderStatus(message: string, detail?: string) {
  root.render(
    <StrictMode>
      <div className="bootstrap-screen">
        <div className="bootstrap-screen__title">{message}</div>
        {detail ? <div className="bootstrap-screen__detail">{detail}</div> : null}
      </div>
    </StrictMode>,
  );
}

async function bootstrap() {
  renderStatus("Loading Rollio UI...");

  try {
    const runtimeConfig = await loadRuntimeConfig();
    root.render(
      <StrictMode>
        {runtimeConfig.mode === "setup" ? (
          <SetupApp runtimeConfig={runtimeConfig} />
        ) : (
          <App runtimeConfig={runtimeConfig} />
        )}
      </StrictMode>,
    );
  } catch (error) {
    renderStatus(
      "Failed to start Rollio UI",
      error instanceof Error ? error.message : String(error),
    );
  }
}

void bootstrap();
