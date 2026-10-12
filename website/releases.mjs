export const repository = 'https://github.com/birkmann/zugriff';
export const filenames = {
  linux: 'zugriff-linux-x86_64.tar.gz',
  windows: 'zugriff-windows-x86_64.zip',
  macos: 'zugriff-macos-arm64.dmg',
};
export function downloads(release) {
  if (!release || release.draft || release.prerelease || !Array.isArray(release.assets)) return {};
  return Object.fromEntries(Object.entries(filenames).flatMap(([platform, filename]) => {
    const asset = release.assets.find(a => a.name === filename && a.state === 'uploaded' && a.size > 0);
    if (!asset) return [];
    try {
      const url = new URL(asset.browser_download_url);
      if (url.origin !== 'https://github.com' || !url.pathname.startsWith('/birkmann/zugriff/releases/download/')) return [];
      return [[platform, { url: url.href, size: Math.round(asset.size / 1024 / 1024) }]];
    } catch { return []; }
  }));
}
