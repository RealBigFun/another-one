import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import fs from "node:fs/promises";
import path from "node:path";

const PI_SESSION_CAPTURE_ENV = "ANOTHER_ONE_PI_SESSION_CAPTURE";

export default function anotherOnePiSessionCapture(pi: ExtensionAPI) {
    pi.on("session_start", async (event, ctx) => {
        const capturePath = process.env[PI_SESSION_CAPTURE_ENV];
        if (!capturePath) return;

        await fs.mkdir(path.dirname(capturePath), { recursive: true });

        const payload = JSON.stringify({
            session_id: ctx.sessionManager.getSessionId(),
            reason: event.reason,
        });

        try {
            await fs.writeFile(capturePath, payload, { flag: "wx" });
        } catch (error) {
            if ((error as { code?: string }).code !== "EEXIST") throw error;
        }
    });
}
