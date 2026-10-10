import { invoke } from "@tauri-apps/api/core";

/** Local IPC boundary. Platform page commands cannot be called through this client. */
export type DesktopCommand =
  | "workspace"
  | "import_roster"
  | "prepare_input_version"
  | "accept_input_version"
  | "preview_legacy"
  | "migrate_legacy"
  | "open_browser"
  | "run_queue"
  | "pause_queue"
  | "resume_queue"
  | "cancel_queue"
  | "review_task"
  | "record_search_scope"
  | "run_step"
  | "adopt_file"
  | "preview_source_file"
  | "lookup_doi_source"
  | "source_reuse_options"
  | "preview_reused_source"
  | "source_file_page"
  | "locate_source_records"
  | "attach_source_file"
  | "source_browser_state"
  | "save_source_site"
  | "open_source_browser"
  | "focus_source_browser"
  | "close_source_browser"
  | "preview_source_download"
  | "cnki_source"
  | "export_report"
  | "open_folder"
  | "ai_settings"
  | "save_ai_settings"
  | "preview_legacy_api_key"
  | "import_legacy_api_key"
  | "classify_task"
  | "run_ai_queue"
  | "resume_ai_queue"
  | "pause_ai_queue"
  | "cancel_ai_queue"
  | "prepare_material_batch"
  | "resume_material_batch"
  | "cancel_material_batch"
  | "submission_options"
  | "prepare_submission"
  | "export_submission_bundle"
  | "templates"
  | "register_template"
  | "fill_template";

export function callDesktop<T>(
  command: DesktopCommand,
  args?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>(command, args);
}
