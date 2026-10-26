// Keep the theme's mobile controls available to keyboard and screen-reader users.
document.addEventListener('DOMContentLoaded', () => {
  const controls = [
    ['__drawer', 'Open navigation', '.md-sidebar--primary', 'kiroha-navigation'],
    ['__search', 'Search documentation', '.md-search', 'kiroha-search'],
  ];
  for (const [name, label, selector, targetId] of controls) {
    const trigger = document.querySelector(`.md-header label[for="${name}"]`);
    const toggle = document.getElementById(name);
    const target = document.querySelector(selector);
    if (!trigger || !toggle || !target) continue;
    target.id = targetId;
    trigger.setAttribute('role', 'button');
    trigger.setAttribute('tabindex', '0');
    trigger.setAttribute('aria-label', label);
    trigger.setAttribute('aria-controls', targetId);
    trigger.addEventListener('keydown', event => {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        event.stopPropagation();
        trigger.click();
      }
    });
    if (name === '__drawer') {
      const update = () => trigger.setAttribute('aria-expanded', String(toggle.checked));
      toggle.addEventListener('change', update);
      update();
      document.addEventListener('keydown', event => {
        if (event.key === 'Escape' && toggle.checked) {
          toggle.checked = false;
          toggle.dispatchEvent(new Event('change'));
          trigger.focus();
        }
      });
    }
  }
});
