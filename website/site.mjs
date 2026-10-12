import { downloads } from './releases.mjs';
const status = document.querySelector('#release-status');
if (document.documentElement.dataset.releaseAccess !== 'public') {
  status.textContent = 'Private preview. Sign in to GitHub with repository access to view downloads.';
  document.querySelector('#release-label').textContent = 'PRIVATE PREVIEW';
} else try {
  const response = await fetch('https://api.github.com/repos/birkmann/zugriff/releases/latest', {
    headers: { Accept: 'application/vnd.github+json' }, signal: AbortSignal.timeout(8000),
  });
  if (response.status === 404) {
    status.textContent = 'No published release was found. Check GitHub Releases for available downloads.';
  } else {
    if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
    const release = await response.json();
    const assets = downloads(release);
    document.querySelector('#release-label').textContent = release.tag_name || 'LATEST RELEASE';
    status.textContent = Object.keys(assets).length ? 'Latest release ready. Choose your platform below.' : 'Desktop packages are not available in the latest release yet.';
    for (const link of document.querySelectorAll('[data-platform]')) {
      const asset = assets[link.dataset.platform];
      if (!asset) continue;
      link.href = asset.url;
      link.classList.add('available');
      link.textContent = `DOWNLOAD · ${asset.size} MB`;
      link.setAttribute('aria-label', `Download ZUGRIFF for ${link.dataset.platform}, ${asset.size} megabytes`);
    }
  }
} catch {
  status.textContent = 'Release information is temporarily unavailable. Check GitHub Releases for downloads.';
}
