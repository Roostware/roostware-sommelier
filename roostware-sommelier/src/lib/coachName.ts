// What the AI helper is called on screen. Only the text people see uses this: the stored
// `role: "coach"` of saved messages, the settings keys and the file names stay as they are.
// The backend's system prompt (src-tauri/src/coach/prompt.rs) also introduces the model by this
// name; keep the two in step.

export const COACH_NAME = "Vin";
