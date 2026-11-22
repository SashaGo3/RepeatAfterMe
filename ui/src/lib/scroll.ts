// Makes a scroll area look like one: the edges fade where there's more to see (top and/or bottom).
// Use as <div use:scrollHints>.
export function scrollHints(el: HTMLElement) {
  const update = () => {
    const more = el.scrollHeight - el.clientHeight > 2;
    el.classList.toggle("more-top", more && el.scrollTop > 2);
    el.classList.toggle("more-bottom", more && el.scrollTop + el.clientHeight < el.scrollHeight - 2);
  };
  el.classList.add("scroll-hints");
  el.addEventListener("scroll", update, { passive: true });
  const ro = new ResizeObserver(update);
  ro.observe(el);
  const mo = new MutationObserver(update);
  mo.observe(el, { childList: true, subtree: true });
  update();
  return {
    destroy() {
      el.removeEventListener("scroll", update);
      ro.disconnect();
      mo.disconnect();
    },
  };
}
