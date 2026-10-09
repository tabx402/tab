import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpRight, Pause, Play } from "lucide-react";

const films = [
  { id: "setup", name: "create an agent", description: "Choose a task, tools, and schedule. Preview ends before registration.", href: "/account" },
  { id: "activity", name: "follow the log", description: "Browse public runs and agent activity. Recorded walkthrough.", href: "/activity" },
  { id: "tools", name: "choose the tools", description: "Check available tools and their connection status.", href: "/providers" },
];
export function AppFilms({ id }: { id: "setup" | "activity" | "tools" }) {
  const [entered, setEntered] = useState(false), [visible, setVisible] = useState(false), [playing, setPlaying] = useState(false), [wantPlay, setWantPlay] = useState(true), [error, setError] = useState(false);
  const [reduced, setReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [manualPlay, setManualPlay] = useState(false);
  const frame = useRef<HTMLDivElement>(null), video = useRef<HTMLVideoElement>(null), visibleRef = useRef(false), automaticPause = useRef(false);
  const film = films.find(item => item.id === id)!;
  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const change = () => { setReduced(media.matches); if (media.matches) setManualPlay(false); };
    media.addEventListener("change", change);
    const observer = new IntersectionObserver(([entry]) => {
      visibleRef.current = entry.isIntersecting; setVisible(entry.isIntersecting);
      if (entry.isIntersecting) setEntered(true);
    }, { threshold: .2 });
    if (frame.current) observer.observe(frame.current);
    return () => { observer.disconnect(); media.removeEventListener("change", change); };
  }, []);
  useEffect(() => {
    const player = video.current;
    if (!player) return;
    const pause = () => { automaticPause.current = true; player.pause(); };
    const start = () => { if (visibleRef.current && !document.hidden && (!reduced || manualPlay) && wantPlay) { automaticPause.current = false; void player.play().catch(() => setPlaying(false)); } else pause(); };
    const timer = window.setTimeout(start, 1300);
    if (!visible || (reduced && !manualPlay) || !wantPlay) pause();
    document.addEventListener("visibilitychange", start);
    return () => { clearTimeout(timer); document.removeEventListener("visibilitychange", start); };
  }, [entered, visible, reduced, wantPlay, manualPlay]);
  return (
    <div className={`embedded-film embedded-film-${id}`}>
      <div ref={frame} className="film-frame" data-entered={entered} aria-label={`${film.name} demonstration`}>
        <svg className="film-ink" viewBox="0 0 1000 700" fill="none" preserveAspectRatio="none" aria-hidden="true"><path pathLength="1" d="M20 16C230 10 750 18 982 13L985 681C745 690 242 684 16 688L13 22L20 16" /><path pathLength="1" d="M1 5L1 55M5 1L58 1M942 699L998 699L998 648" /></svg>
        <div className="film-player"><video key={film.id} ref={video} src={entered ? `/media/${film.id}-bnb-walkthrough.mp4` : undefined} poster={`/media/${film.id}-bnb-poster.webp`} muted playsInline loop preload="none" aria-label={`${film.name} walkthrough`} onError={() => setError(true)} onPlay={() => { automaticPause.current = false; setPlaying(true); setManualPlay(true); }} onPause={() => { setPlaying(false); if (visibleRef.current && !automaticPause.current && !document.hidden) setWantPlay(false); }} /><span className="film-snapshot">{film.id === "setup" ? "setup demonstration" : "recorded interface preview"}</span></div>
      </div>
      <div className="film-caption"><p>{error ? "The video could not load. Open the app to try the flow." : film.description}</p><div><button className="film-toggle" aria-label={`${playing ? "Pause" : "Play"} ${film.name} walkthrough`} onClick={() => {
        if (!video.current) return;
        if (playing) { setWantPlay(false); video.current.pause(); }
        else { setWantPlay(true); setManualPlay(true); automaticPause.current = false; void video.current.play().catch(() => setError(true)); }
      }}>{playing ? <Pause size={15} /> : <Play size={15} />}</button><Link className="text-link" to={film.href}>try it <ArrowUpRight size={15} /></Link></div></div>
    </div>
  );
}
