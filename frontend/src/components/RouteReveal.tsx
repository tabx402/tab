import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import { useLocation } from "react-router-dom";

// Reveal independent pieces; avoid fading a parent and all its children together.
const selector = [
  ".landing-hero-copy > *", ".landing-scroll", ".landing-copy > *", ".landing-section-heading > *", ".landing-control-grid > *", ".landing-tool-list > *", ".landing-questions > h2", ".landing-faq > *", ".landing-end > *",
  ".landing-doc-links > *", ".docs-content > section", ".end-blooms",
  ".hero > div:first-child > *", ".hero-art",
  ".page-intro > div:first-child > *", ".page-intro > :not(div:first-child)",
  ".section-line", ".filter-row > *", ".note-line", ".back-link", ".metric", ".chart-panel > *", ".loop-panel > :not(.flow-list)", ".flow-list > *",
  ".agents-panel > .panel-heading", ".agents-panel .empty", ".agents-panel thead", ".agents-panel tbody tr", ".terminal > *", ".explain-section > *",
  ".provider-card", ".backing-grid > *", ".detail-grid > *",
  ".account-grid > *", ".account-welcome > *", ".agent-builder", ".workspace-heading", ".owned-agent", ".agent-controls", ".account-signin", ".live-activity > .panel-heading", ".overview-actions > *",
  ".protocol-copy > section", ".registry-panel > *",
].join(", ");

export function RouteReveal({ children, ready }: { children: ReactNode; ready: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const readyRef = useRef(ready);
  const refresh = useRef<(() => void) | null>(null);
  readyRef.current = ready;
  const { pathname } = useLocation();

  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const seen = new Set<HTMLElement>();
    const waiting = new Set<HTMLElement>();
    let frame = 0;
    const observer = new IntersectionObserver((entries) => {
      const entering = entries.filter((entry) => entry.isIntersecting);
      // Stagger siblings entering in one scroll pass, without a long global queue.
      const groups = new Map<Element | null, number>();
      entering.forEach((entry) => {
        const target = entry.target as HTMLElement;
        const index = groups.get(target.parentElement) ?? 0;
        groups.set(target.parentElement, index + 1);
        if (!target.dataset.entryDelay) target.style.setProperty("--reveal-delay", `${Math.min(index, 3) * 90}ms`);
        target.classList.add("is-visible");
        observer.unobserve(target);
      });
    }, { threshold: 0.08, rootMargin: "0px 0px -20px 0px" });

    const scan = () => {
      for (const target of element.querySelectorAll<HTMLElement>(selector)) {
        if (seen.has(target)) continue;
        seen.add(target);
        target.classList.add("reveal-item");
        const hero = target.closest(".hero, .page-intro, .landing-hero");
        if (hero) {
          const parts = [...hero.querySelectorAll<HTMLElement>(selector)];
          const delay = hero.matches(".landing-hero") ? 650 + parts.indexOf(target) * 180 : target.matches(".hero-art, .bird") ? 440 : 70 + parts.indexOf(target) * 90;
          target.dataset.entryDelay = String(delay);
          target.style.setProperty("--reveal-delay", `${delay}ms`);
        }
        waiting.add(target);
      }
      for (const target of waiting) {
        if (!media.matches && target.closest('.landing[data-artwork-ready="false"]')) continue;
        // Editorial copy and SVG art enter immediately, independently of API data.
        if (!media.matches && !readyRef.current && !target.closest(".hero, .page-intro, .landing-hero")) continue;
        waiting.delete(target);
        if (media.matches) target.classList.add("is-visible");
        else observer.observe(target);
      }
    };
    refresh.current = scan;
    const mutations = new MutationObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(scan);
    });
    scan();
    mutations.observe(element, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-artwork-ready"] });
    const onMotion = () => {
      if (media.matches) {
        observer.disconnect();
        seen.forEach((target) => target.classList.add("is-visible"));
        waiting.clear();
      }
    };
    media.addEventListener("change", onMotion);
    return () => {
      observer.disconnect(); mutations.disconnect(); cancelAnimationFrame(frame);
      media.removeEventListener("change", onMotion);
      refresh.current = null;
      seen.forEach((target) => {
        target.classList.remove("reveal-item", "is-visible");
        target.style.removeProperty("--reveal-delay");
        delete target.dataset.entryDelay;
      });
    };
  }, [pathname]);

  useLayoutEffect(() => { refresh.current?.(); }, [ready]);
  return <div className="route-content" ref={root}>{children}</div>;
}
