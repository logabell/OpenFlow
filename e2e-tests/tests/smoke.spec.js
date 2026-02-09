import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";

import { Builder, By, Capabilities, until } from "selenium-webdriver";

const appPath = process.env.OPENFLOW_E2E_APP_PATH;
if (!appPath) {
  throw new Error("OPENFLOW_E2E_APP_PATH env var is required");
}
if (!fs.existsSync(appPath)) {
  throw new Error(`Application not found: ${appPath}`);
}

const webdriverUrl = process.env.TAURI_WEBDRIVER_URL ?? "http://127.0.0.1:4444/";
const defaultTauriDriverPath = path.resolve(os.homedir(), ".cargo", "bin", "tauri-driver");
const tauriDriverPath =
  process.env.TAURI_DRIVER_PATH ??
  (fs.existsSync(defaultTauriDriverPath) ? defaultTauriDriverPath : "tauri-driver");

async function waitForPort(host, port, timeoutMs) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      await new Promise((resolve, reject) => {
        const socket = net.connect(port, host);
        const onError = (err) => {
          socket.destroy();
          reject(err);
        };
        socket.once("error", onError);
        socket.once("connect", () => {
          socket.end();
          resolve();
        });
        socket.setTimeout(750, () => onError(new Error("timeout")));
      });
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  throw new Error(`Timed out waiting for ${host}:${port}`);
}

let driver;
let tauriDriver;
let tauriDriverExitExpected = false;
let wrapperPath;

function shSingleQuote(value) {
  return `'${String(value).replace(/'/g, `'"'"'`)}'`;
}

before(async function () {
  this.timeout(180000);

  // Wrap the application so we can force deterministic test behavior regardless
  // of how `tauri-driver` propagates environment variables to the spawned app.
  const appDir = path.dirname(appPath);
  const libDir = path.join(appDir, "lib");
  wrapperPath = path.join(os.tmpdir(), `openflow-e2e-wrapper-${process.pid}-${Date.now()}.sh`);
  const wrapper = [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    "",
    // Default to disabling warmup/model auto-download in E2E, but allow overrides.
    'export OPENFLOW_DISABLE_ASR_WARMUP="${OPENFLOW_DISABLE_ASR_WARMUP:-1}"',
    'export OPENFLOW_DISABLE_MODEL_AUTODOWNLOAD="${OPENFLOW_DISABLE_MODEL_AUTODOWNLOAD:-1}"',
    'export OPENFLOW_DISABLE_UPDATE_CHECK="${OPENFLOW_DISABLE_UPDATE_CHECK:-1}"',
    "",
    `APP_BIN=${shSingleQuote(appPath)}`,
    `LIB_DIR=${shSingleQuote(libDir)}`,
    "if [ -d \"$LIB_DIR\" ]; then",
    '  export LD_LIBRARY_PATH="$LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"',
    "fi",
    "",
    'exec "$APP_BIN" "$@"',
    "",
  ].join("\n");
  fs.writeFileSync(wrapperPath, wrapper, { encoding: "utf8" });
  fs.chmodSync(wrapperPath, 0o755);

  tauriDriver = spawn(tauriDriverPath, [], {
    stdio: ["ignore", process.stdout, process.stderr],
  });

  tauriDriver.on("error", (error) => {
    console.error("tauri-driver error:", error);
    process.exit(1);
  });

  tauriDriver.on("exit", (code) => {
    if (!tauriDriverExitExpected) {
      console.error("tauri-driver exited unexpectedly with code:", code);
      process.exit(1);
    }
  });

  const url = new URL(webdriverUrl);
  const host = url.hostname || "127.0.0.1";
  const port = Number(url.port || 4444);
  await waitForPort(host, port, 15000);

  const capabilities = new Capabilities();
  capabilities.set("tauri:options", { application: wrapperPath });
  capabilities.setBrowserName("wry");

  driver = await new Builder()
    .withCapabilities(capabilities)
    .usingServer(webdriverUrl)
    .build();
});

after(async function () {
  tauriDriverExitExpected = true;
  try {
    await driver?.quit();
  } finally {
    tauriDriver?.kill();
    try {
      if (wrapperPath && fs.existsSync(wrapperPath)) {
        fs.unlinkSync(wrapperPath);
      }
    } catch {
      // ignore
    }
  }
});

describe("OpenFlow UI", function () {
  it("renders the dashboard", async function () {
    this.timeout(180000);

    const header = await driver.wait(until.elementLocated(By.css("header h1")), 60000);
    const title = await header.getText();
    assert.match(title, /OpenFlow/);

    const settingsButton = await driver.wait(
      until.elementLocated(By.xpath("//button[contains(., 'Settings')]")),
      60000,
    );
    const settingsText = await settingsButton.getText();
    assert.match(settingsText, /Settings/);
  });

  it("opens Settings", async function () {
    this.timeout(180000);

    const settingsButton = await driver.wait(
      until.elementLocated(By.xpath("//button[contains(., 'Settings')]")),
      60000,
    );
    try {
      await settingsButton.click();
    } catch (error) {
      // Some WebKit WebDriver builds (notably on Fedora) don't support native element clicks.
      // Fall back to a DOM click so we can still validate basic UI wiring.
      if (error?.name !== "UnsupportedOperationError") {
        throw error;
      }
      await driver.executeScript("arguments[0].click()", settingsButton);
    }

    const panelHeader = await driver.wait(
      until.elementLocated(By.xpath("//h2[contains(., 'Settings')]")),
      60000,
    );
    const text = await panelHeader.getText();
    assert.match(text, /Settings/);
  });
});
