// Trunk "initializer": hooks of the download of the WebAssembly file (about 18 MB, 4.7 MB with Brotli) used by the loading screen of
// index.html. Nothing here is needed to run the application.
export default function initializer() {
  const bar = () => document.getElementById('loading-bar');
  const text = () => document.getElementById('loading-size');
  const mb = (n) => (n / 1048576).toFixed(1);
  const unit = document.documentElement.lang === 'fr' ? 'Mo' : 'MB';
  return {
    onStart: () => {},
    onProgress: ({ current, total }) => {
      const b = bar();
      const t = text();
      // the total is the size on the wire (compressed) when the server compresses: the stream counts what the browser has unpacked, so it
      // can pass the total; the bar then stops at 99 % until the page starts, and the size shows what has been received
      if (b && total > 0) b.style.width = `${Math.min(99, Math.round((current / total) * 100))}%`;
      if (t) t.textContent = total > 0 && current <= total ? `${mb(current)} / ${mb(total)} ${unit}` : `${mb(current)} ${unit}`;
    },
    onComplete: () => {
      const b = bar();
      if (b) b.style.width = '100%';
      const t = text();
      if (t) t.textContent = '';
    },
    onSuccess: () => {},
    onFailure: (error) => {
      // the page cannot be fetched or compiled (network cut, file damaged, a browser without WebAssembly features it needs)
      if (window.inkboardFail) window.inkboardFail('other', String((error && error.stack) || error));
    },
  };
}
