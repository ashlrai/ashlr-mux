import { host } from "./host";

const MARKDOWN_EXTENSIONS = new Set(["md", "markdown", "mdown", "mkd", "mkdn", "mdx"]);

export type FileExplorerEntryKind = "directory" | "file" | "symlink" | "other";

export interface FileExplorerEntry {
  name: string;
  path: string;
  relativePath: string;
  kind: FileExplorerEntryKind;
  size?: number | null;
}

export interface FileExplorerListRequest {
  rootPath: string;
  relativePath?: string | null;
  showHidden?: boolean | null;
}

export interface FileExplorerOpenRequest {
  path: string;
  preferredEditor?: string | null;
}

export interface FileExplorerOpenReply {
  opened: boolean;
  message: string;
}

export interface FileExplorerReadFileRequest {
  path: string;
}

export interface FileExplorerReadFileReply {
  path: string;
  content: string;
  size: number;
}

export interface FileExplorerWriteFileRequest {
  path: string;
  content: string;
}

export interface FileExplorerWriteFileReply {
  saved: boolean;
  message: string;
  size: number;
}

export function listFileExplorerDirectory(
  request: FileExplorerListRequest,
): Promise<FileExplorerEntry[]> {
  return host.invoke<FileExplorerEntry[]>("file_explorer_list_directory", { request });
}

export function openFileExplorerPath(
  request: FileExplorerOpenRequest,
): Promise<FileExplorerOpenReply> {
  return host.invoke<FileExplorerOpenReply>("file_explorer_open_path", { request });
}

export function readFileExplorerFile(
  request: FileExplorerReadFileRequest,
): Promise<FileExplorerReadFileReply> {
  return host.invoke<FileExplorerReadFileReply>("file_explorer_read_file", { request });
}

export function writeFileExplorerFile(
  request: FileExplorerWriteFileRequest,
): Promise<FileExplorerWriteFileReply> {
  return host.invoke<FileExplorerWriteFileReply>("file_explorer_write_file", {
    request,
  });
}

export function isMarkdownFilePath(path: string): boolean {
  const name = path.split(/[\\/]/).pop() ?? path;
  const index = name.lastIndexOf(".");
  if (index < 0 || index === name.length - 1) {
    return false;
  }
  return MARKDOWN_EXTENSIONS.has(name.slice(index + 1).toLowerCase());
}

export function fileExplorerEntryIcon(kind: FileExplorerEntryKind): string {
  switch (kind) {
    case "directory":
      return "folder";
    case "symlink":
      return "link";
    case "file":
      return "file";
    case "other":
      return "item";
  }
}

export function fileExplorerParentPath(relativePath: string): string | null {
  const normalized = relativePath.replaceAll("\\", "/").replace(/\/+$/, "");
  const index = normalized.lastIndexOf("/");
  if (index <= 0) {
    return normalized === "" ? null : "";
  }
  return normalized.slice(0, index);
}

export function fileExplorerSizeLabel(size: number | null | undefined): string {
  if (size == null) {
    return "";
  }
  if (size < 1024) {
    return `${size} B`;
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(size < 10 * 1024 ? 1 : 0)} KB`;
  }
  return `${(size / (1024 * 1024)).toFixed(size < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}
