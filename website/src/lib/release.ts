// The latest release, read from GitHub once at build time so the page never shows a stale version.
// If GitHub can't be reached, the last known release is used instead.

const REPO = 'itsofficially-sreyash/Morph';
export const REPO_URL = `https://github.com/${REPO}`;
export const RELEASES_URL = `${REPO_URL}/releases/latest`;

export type Kind = 'windows' | 'deb' | 'rpm' | 'appimage';

export interface Asset {
  url: string;
  /** Size in MB, rounded. */
  mb: number;
}

export interface Release {
  version: string;
  assets: Record<Kind, Asset>;
}

const PATTERNS: Record<Kind, RegExp> = {
  windows: /_x64-setup\.exe$/,
  deb: /_amd64\.deb$/,
  rpm: /\.x86_64\.rpm$/,
  appimage: /_amd64\.AppImage$/,
};

function fallback(): Release {
  const v = '0.1.0';
  const base = `${REPO_URL}/releases/download/v${v}`;
  return {
    version: v,
    assets: {
      windows: { url: `${base}/Morph_${v}_x64-setup.exe`, mb: 43 },
      deb: { url: `${base}/Morph_${v}_amd64.deb`, mb: 64 },
      rpm: { url: `${base}/Morph-${v}-1.x86_64.rpm`, mb: 64 },
      appimage: { url: `${base}/Morph_${v}_amd64.AppImage`, mb: 133 },
    },
  };
}

let cached: Promise<Release> | undefined;

export function latestRelease(): Promise<Release> {
  cached ??= load();
  return cached;
}

async function load(): Promise<Release> {
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: AbortSignal.timeout(8000),
    });
    if (!res.ok) throw new Error(`GitHub answered ${res.status}`);
    const data = (await res.json()) as {
      tag_name: string;
      assets: { name: string; browser_download_url: string; size: number }[];
    };
    const assets = {} as Record<Kind, Asset>;
    for (const kind of Object.keys(PATTERNS) as Kind[]) {
      const a = data.assets.find((x) => PATTERNS[kind].test(x.name));
      if (!a) throw new Error(`no ${kind} asset in ${data.tag_name}`);
      assets[kind] = { url: a.browser_download_url, mb: Math.round(a.size / 1_000_000) };
    }
    return { version: data.tag_name.replace(/^v/, ''), assets };
  } catch (e) {
    console.warn(`[release] using the last known release: ${(e as Error).message}`);
    return fallback();
  }
}
