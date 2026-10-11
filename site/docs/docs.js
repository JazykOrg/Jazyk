// Enhancement only: articles and navigation work without this script.
document.querySelectorAll('.copy-button').forEach(button => {
  button.hidden = false;
  button.addEventListener('click', async () => {
    const text = button.closest('.code-block').querySelector('code').textContent;
    try {
      await navigator.clipboard.writeText(text);
      button.textContent = 'Copied';
      document.getElementById('copy-status').textContent = 'Example copied to clipboard.';
    } catch {
      button.textContent = 'Select to copy';
      const range = document.createRange();
      range.selectNodeContents(button.closest('.code-block').querySelector('code'));
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.getElementById('copy-status').textContent = 'Clipboard unavailable. Example selected; use your copy shortcut.';
    }
    setTimeout(() => { button.textContent = 'Copy'; }, 2500);
  });
});
if ('IntersectionObserver' in window) {
  const visible = new Set();
  const sections = [...document.querySelectorAll('.article-section')];
  const observer = new IntersectionObserver(entries => {
    entries.forEach(entry => entry.isIntersecting ? visible.add(entry.target) : visible.delete(entry.target));
    const current = sections.find(section => visible.has(section));
    if (!current) return;
    document.querySelectorAll('.toc a').forEach(link => {
      if (link.getAttribute('href') === '#' + current.id) link.setAttribute('aria-current', 'location');
      else link.removeAttribute('aria-current');
    });
  }, { rootMargin: '-100px 0px -55% 0px' });
  sections.forEach(section => observer.observe(section));
}
