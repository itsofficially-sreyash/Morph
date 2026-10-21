// Placeholder catalog for the radial menu until the Rust `actions` registry lands (step 4).

export type CategoryId = "image" | "video" | "audio" | "pdf" | "document";

export interface MenuAction {
  id: string;
  label: string;
  hint: string;
  /** Extensions this action is limited to (Rust: `Action::only`). */
  only?: string[];
}

export interface MenuCategory {
  id: CategoryId;
  label: string;
  /** Hue (degrees) used to tint this category's ring. */
  hue: number;
  extensions: string[];
  actions: MenuAction[];
}

const convert = (to: string): MenuAction => ({
  id: `convert:${to}`,
  label: to.toUpperCase(),
  hint: `Convert to ${to.toUpperCase()}`,
});

/** Documents that can become DOCX/ODT/TXT; slides and spreadsheets only become PDFs. */
const TEXT_DOCS = ["docx", "doc", "odt", "rtf", "txt"];

export const CATEGORIES: MenuCategory[] = [
  {
    id: "image",
    label: "Image",
    hue: 262,
    extensions: ["png", "jpg", "jpeg", "webp", "avif", "heic", "bmp", "tiff", "gif", "ico"],
    actions: [
      convert("png"),
      convert("jpg"),
      convert("webp"),
      convert("avif"),
      { id: "resize", label: "Resize", hint: "Resize by size or percentage" },
      { id: "crop", label: "Crop", hint: "Crop to a region you draw" },
      { id: "compress", label: "Shrink", hint: "Compress to a smaller file" },
      { id: "to-pdf", label: "PDF", hint: "Put images into one PDF" },
    ],
  },
  {
    id: "video",
    label: "Video",
    hue: 335,
    extensions: ["mp4", "mkv", "webm", "mov", "avi"],
    actions: [
      convert("mp4"),
      convert("webm"),
      convert("gif"),
      { id: "compress", label: "Shrink", hint: "Compress with a quality preset" },
      { id: "extract-audio", label: "Audio", hint: "Extract the soundtrack" },
      { id: "mute", label: "Mute", hint: "Remove the sound" },
    ],
  },
  {
    id: "audio",
    label: "Audio",
    hue: 172,
    extensions: ["mp3", "wav", "flac", "ogg", "m4a", "opus"],
    actions: [
      convert("mp3"),
      convert("wav"),
      convert("flac"),
      convert("m4a"),
      { id: "normalize", label: "Level", hint: "Normalize loudness" },
    ],
  },
  {
    id: "pdf",
    label: "PDF",
    hue: 12,
    extensions: ["pdf"],
    actions: [
      { id: "merge", label: "Merge", hint: "Combine PDFs in the order you choose" },
      { id: "split", label: "Split", hint: "Every page, or ranges like 1-3, 5" },
      { id: "compress", label: "Shrink", hint: "Compress images inside" },
      { id: "to-images", label: "Images", hint: "Export each page as PNG" },
      { id: "rotate", label: "Rotate", hint: "Rotate all pages 90°" },
    ],
  },
  {
    id: "document",
    label: "Docs",
    hue: 210,
    extensions: ["docx", "doc", "odt", "pptx", "ppt", "odp", "xlsx", "xls", "ods", "rtf", "txt"],
    actions: [
      { id: "to-pdf", label: "PDF", hint: "Save as PDF" },
      { id: "convert:docx", label: "DOCX", hint: "Save as a Word document", only: TEXT_DOCS },
      { id: "convert:odt", label: "ODT", hint: "Save as OpenDocument text", only: TEXT_DOCS },
      { id: "convert:txt", label: "TXT", hint: "Save as plain text", only: TEXT_DOCS },
    ],
  },
];

/** Categories that apply to at least one of the given file paths, in catalog order. */
export function categoriesFor(files: string[]): MenuCategory[] {
  const exts = new Set(files.map((f) => f.split(".").pop()?.toLowerCase() ?? ""));
  return CATEGORIES.filter((c) => c.extensions.some((e) => exts.has(e)));
}

const ext = (path: string) => path.split(".").pop()?.toLowerCase() ?? "";

/** Whether `action` of `category` works on this file (Rust: `Action::accepts`). */
export function actionAccepts(category: MenuCategory, action: MenuAction, path: string): boolean {
  return category.extensions.includes(ext(path)) && (!action.only || action.only.includes(ext(path)));
}

/** The actions offered for these files: those that apply to at least one (Rust: `actions_for`). */
export function actionsFor(category: MenuCategory, files: string[]): MenuAction[] {
  return category.actions.filter((a) => files.some((f) => actionAccepts(category, a, f)));
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
