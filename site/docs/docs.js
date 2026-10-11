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

// This example illustrates change contracts. It does not execute the compiler.
const changeSelect = document.getElementById('change-kind');
if (changeSelect) {
  changeSelect.disabled = false;
  const examples = {
    rule: {
      title: 'The cancellation rule',
      detail: 'Changing the cancellation day means reconciling the affected section and rejudging its requirements. Derived criteria may become stale. Bound tests and implementation evidence need review.',
      steps: ['Prose edit', 'Reconcile', 'Review evidence']
    },
    move: {
      title: 'A section moved unchanged',
      detail: 'When alignment recognizes the same section in a new location, the compiler rewrites its stored references mechanically. That move does not mark the section dirty or require another extraction session.',
      steps: ['Section move', 'Align anchors', 'Preserve facts']
    },
    source: {
      title: 'The implementation changed',
      detail: 'Changing product files makes their recorded verification evidence stale. Verify again. A failure with implementing files is a finding to inspect; it is not automatically a request to regenerate all code.',
      steps: ['Source edit', 'Evidence stale', 'Verify again']
    },
    proposal: {
      title: 'An inferred fact was accepted',
      detail: 'Accepting its ratification proposal inserts the prose and flips the fact’s provenance to quote in one changeset. The fact keeps its ID. Changed obligations can still open downstream judgment or evidence work.',
      steps: ['Accept proposal', 'Prose + graph', 'Quoted fact']
    }
  };
  changeSelect.addEventListener('change', () => {
    const example = examples[changeSelect.value];
    if (!example) return;
    const result = document.getElementById('change-result');
    result.querySelector('h3').textContent = example.title;
    result.querySelector('p').textContent = example.detail;
    const flow = result.querySelector('.workflow');
    flow.replaceChildren();
    example.steps.forEach((step, index) => {
      if (index) {
        const arrow = document.createElement('span');
        arrow.textContent = '→';
        arrow.setAttribute('aria-hidden', 'true');
        flow.append(arrow);
      }
      const label = document.createElement('span');
      label.textContent = step;
      flow.append(label);
    });
  });
}
