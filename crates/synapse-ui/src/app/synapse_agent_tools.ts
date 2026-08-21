import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const bridgeUrl = process.env.SYNAPSE_AGENT_BRIDGE_URL;
const bridgeToken = process.env.SYNAPSE_AGENT_BRIDGE_TOKEN;

async function callBridge(operation: string, input: object, signal: AbortSignal) {
  if (!bridgeUrl || !bridgeToken) {
    throw new Error("Synapse workspace bridge is unavailable");
  }
  const response = await fetch(`${bridgeUrl}/v1/workspace`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${bridgeToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ operation, ...input }),
    signal,
  });
  const body = await response.json();
  if (!response.ok || !body.ok) {
    throw new Error(body?.error?.message ?? `Synapse bridge failed (${response.status})`);
  }
  return {
    content: [{ type: "text" as const, text: JSON.stringify(body.data, null, 2) }],
    details: body.data,
  };
}

export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "synapse_todo_list",
    label: "List Synapse todos",
    description: "List all todos in the running Synapse app.",
    parameters: Type.Object({}),
    execute: (_id, input, signal) => callBridge("todo.list", input, signal),
  });
  pi.registerTool({
    name: "synapse_todo_create",
    label: "Create Synapse todo",
    description: "Create a todo in the running Synapse app.",
    parameters: Type.Object({ text: Type.String({ minLength: 1, maxLength: 500 }) }),
    execute: (_id, input, signal) => callBridge("todo.create", input, signal),
  });
  pi.registerTool({
    name: "synapse_todo_update",
    label: "Update Synapse todo",
    description: "Update the text and/or completed state of a Synapse todo.",
    parameters: Type.Object({
      id: Type.Integer({ minimum: 1 }),
      text: Type.Optional(Type.String({ minLength: 1, maxLength: 500 })),
      done: Type.Optional(Type.Boolean()),
    }),
    execute: (_id, input, signal) => callBridge("todo.update", input, signal),
  });
  pi.registerTool({
    name: "synapse_todo_delete",
    label: "Delete Synapse todo",
    description: "Delete a todo from the running Synapse app.",
    parameters: Type.Object({ id: Type.Integer({ minimum: 1 }) }),
    execute: (_id, input, signal) => callBridge("todo.delete", input, signal),
  });
  pi.registerTool({
    name: "synapse_bookmark_list",
    label: "List Synapse bookmarks",
    description: "List all bookmarks in the running Synapse app.",
    parameters: Type.Object({}),
    execute: (_id, input, signal) => callBridge("bookmark.list", input, signal),
  });
  pi.registerTool({
    name: "synapse_bookmark_create",
    label: "Create Synapse bookmark",
    description: "Create an HTTP or HTTPS bookmark in the running Synapse app.",
    parameters: Type.Object({
      url: Type.String({ minLength: 1 }),
      title: Type.Optional(Type.String({ minLength: 1 })),
    }),
    execute: (_id, input, signal) => callBridge("bookmark.create", input, signal),
  });
  pi.registerTool({
    name: "synapse_bookmark_update",
    label: "Update Synapse bookmark",
    description: "Update the title of a Synapse bookmark.",
    parameters: Type.Object({
      id: Type.Integer({ minimum: 1 }),
      title: Type.String({ minLength: 1 }),
    }),
    execute: (_id, input, signal) => callBridge("bookmark.update", input, signal),
  });
  pi.registerTool({
    name: "synapse_bookmark_delete",
    label: "Delete Synapse bookmark",
    description: "Delete a bookmark from the running Synapse app.",
    parameters: Type.Object({ id: Type.Integer({ minimum: 1 }) }),
    execute: (_id, input, signal) => callBridge("bookmark.delete", input, signal),
  });
}
