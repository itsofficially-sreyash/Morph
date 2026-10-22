// The latest release, read once at build time so the page never shows a stale version.
// Sources, in order: the GitHub API (exact sizes; 60 lookups an hour per address, so a shared
// build server can run out), the release's latest.json (no limit; the app's updater reads it too),
// then the last known release.

const REPO = 'itsofficially-sreyash/Morph';
export const REPO_URL = `https://github.com/${REPO}`;
export const RELEASES_URL = `${REPO_URL}/releases/latest`;

/** SourceForge copies each GitHub release into a folder named after its tag. */
const MIRROR = 'https://downloads.sourceforge.net/project/morphconvert';

export type Kind = 'windows' | 'deb' | 'rpm' | 'appimage';

export interface Asset {
  url: string;
  /** The same file on SourceForge, when it's there already (the copy can lag behind GitHub). */
  mirror?: string;
  /** Size in MB, rounded. */
  mb: number;
}

export interface Release {
  version: string;
  assets: Record<Kind, Asset>;
}

const KINDS: Kind[] = ['windows', 'deb', 'rpm', 'appimage'];

const PATTERNS: Record<Kind, RegExp> = {
  windows: /_x64-setup\.exe$/,
  deb: /_amd64\.deb$/,
  rpm: /\.x86_64\.rpm$/,
  appimage: /_amd64\.AppImage$/,
};

/** latest.json platform keys (written by tauri-action). */
const PLATFORMS: Record<Kind, string> = {
  windows: 'windows-x86_64-nsis',
  deb: 'linux-x86_64-deb',
  rpm: 'linux-x86_64-rpm',
  appimage: 'linux-x86_64-appimage',
};

const mb = (bytes: number) => Math.round(bytes / 1_000_000);

function lastKnown(): Release {
  const v = '0.1.0';
  const base = `${REPO_URL}/releases/download/v${v}`;
  return {
    version: v,
    assets: {
      windows: { url: `${base}/Morph_${v}_x64-setup.exe`, mb: 45 },
      deb: { url: `${base}/Morph_${v}_amd64.deb`, mb: 67 },
      rpm: { url: `${base}/Morph-${v}-1.x86_64.rpm`, mb: 67 },
      appimage: { url: `${base}/Morph_${v}_amd64.AppImage`, mb: 139 },
    },
  };
}

let cached: Promise<Release> | undefined;

export function latestRelease(): Promise<Release> {
  cached ??= load();
  return cached;
}

async function load(): Promise<Release> {
  const r = (await fromApi()) ?? (await fromLatestJson()) ?? lastKnown();
  await Promise.all(
    Object.values(r.assets).map(async (a) => {
      const url = `${MIRROR}/v${r.version}/${a.url.split('/').pop()}`;
      if (await mirrored(url)) a.mirror = url;
    }),
  );
  return r;
}

async function fromApi(): Promise<Release | undefined> {
  try {
    const headers: Record<string, string> = { Accept: 'application/vnd.github+json' };
    if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers,
      signal: AbortSignal.timeout(8000),
    });
    if (!res.ok) throw new Error(`GitHub answered ${res.status}`);
    const data = (await res.json()) as {
      tag_name: string;
      assets: { name: string; browser_download_url: string; size: number }[];
    };
    const assets = {} as Record<Kind, Asset>;
    for (const kind of KINDS) {
      const a = data.assets.find((x) => PATTERNS[kind].test(x.name));
      if (!a) throw new Error(`no ${kind} asset in ${data.tag_name}`);
      assets[kind] = { url: a.browser_download_url, mb: mb(a.size) };
    }
    return { version: data.tag_name.replace(/^v/, ''), assets };
  } catch (e) {
    console.warn(`[release] GitHub API: ${(e as Error).message}; trying latest.json`);
    return undefined;
  }
}

async function fromLatestJson(): Promise<Release | undefined> {
  try {
    const res = await fetch(`${RELEASES_URL}/download/latest.json`, { signal: AbortSignal.timeout(8000) });
    if (!res.ok) throw new Error(`latest.json answered ${res.status}`);
    const data = (await res.json()) as { version: string; platforms: Record<string, { url: string }> };
    const known = lastKnown().assets;
    const assets = {} as Record<Kind, Asset>;
    await Promise.all(
      KINDS.map(async (kind) => {
        const url = data.platforms[PLATFORMS[kind]]?.url;
        if (!url) throw new Error(`no ${PLATFORMS[kind]} in latest.json`);
        assets[kind] = { url, mb: (await size(url)) ?? known[kind].mb };
      }),
    );
    return { version: data.version, assets };
  } catch (e) {
    console.warn(`[release] latest.json: ${(e as Error).message}; using the last known release`);
    return undefined;
  }
}

/** Size in MB from the download's headers, without downloading it. */
async function size(url: string): Promise<number | undefined> {
  try {
    const res = await fetch(url, { method: 'HEAD', signal: AbortSignal.timeout(8000) });
    const bytes = Number(res.headers.get('content-length'));
    return res.ok && bytes > 0 ? mb(bytes) : undefined;
  } catch {
    return undefined;
  }
}

/** SourceForge answers a file it has with a redirect to a download server, and one it lacks with 404. */
async function mirrored(url: string): Promise<boolean> {
  try {
    const res = await fetch(url, { method: 'HEAD', redirect: 'manual', signal: AbortSignal.timeout(8000) });
    return res.status >= 200 && res.status < 400;
  } catch {
    console.warn(`[release] couldn't check the mirror for ${url}`);
    return false;
  }
}
