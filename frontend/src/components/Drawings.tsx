const birdShapes = [
    "M7 26C3 19 7 10 16 10C25 9 30 15 28 22L34 28L23 27C17 32 10 31 7 26ZM10 23C12 16 21 16 24 23C20 27 14 28 10 23M28 15L34 18L28 20M13 30L12 35M21 29L22 35",
    "M8 26C5 20 8 9 17 8C26 7 30 15 27 22L33 28L23 26C17 31 11 31 8 26M10 23C13 17 20 16 23 23M28 13L34 16L28 18M14 30L14 35M21 29L22 35",
    "M7 25C4 17 11 10 18 11C25 11 28 16 27 22L34 27L23 27C18 31 10 30 7 25M10 22C15 16 20 19 22 25M28 16L34 19L28 21M13 29L12 35M20 29L21 35",
    "M8 25C5 16 10 9 19 10C27 11 29 17 27 23L33 28L22 27C16 31 10 30 8 25M11 23C13 16 19 17 23 24M28 15L34 18L28 20M14 30L13 35M21 29L22 35",
  ];

export function Sprout({ className = "" }: { className?: string }) {
  return (
    <svg
      className={className}
      width="30"
      height="34"
      viewBox="0 0 40 40"
      fill="none"
      aria-hidden="true"
    >
      <path
        pathLength="1"
        d="M20 35V18M20 26C5 26 5 8 20 18C20 3 38 8 20 26M15 35H25"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function FlyingBirds() {
  return (
    <svg className="landing-birds" viewBox="0 0 600 320" fill="none" aria-hidden="true">
      {["translate(355 52) scale(2.3)", "translate(260 158) scale(1.6)", "translate(460 192) scale(1.25)", "translate(118 42) scale(1.1)", "translate(160 229) scale(.95)", "translate(394 257) scale(.9)"].map((position, index) => (
        <g key={position} transform={position}>
          <g className={`bird-flight flight-${index}`} stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round">
            <path className="flight-body flight-ink" pathLength="1" d="M8 25C5 19 8 11 17 10C25 9 30 15 27 22L35 27L23 26C18 31 11 30 8 25M28 14L35 17L28 19" />
            <g className="flight-wing"><path className="flight-ink" pathLength="1" d="M12 23C5 17 2 8 6 3C16 8 20 14 23 22M12 20L7 10M16 20L11 9M19 21L15 12" /></g>
            <g className="flight-tail"><path className="flight-ink" pathLength="1" d="M23 25L33 30L29 25L35 28L29 22M25 23L32 26" /></g>
            <path className="flight-ink" pathLength="1" d="M13 29L11 32M18 29L17 32M10 24C13 27 16 28 20 26M18 12C21 11 23 12 25 13" />
            <g className="flight-blink"><circle className="flight-eye" cx="24" cy="15" r=".75" fill="currentColor" stroke="none" /></g>
          </g>
        </g>
      ))}
    </svg>
  );
}

export function BotanicalSpray({ className = "" }: { className?: string }) {
  return (
    <svg className={`botanical-spray ${className}`} viewBox="0 0 240 300" fill="none" aria-hidden="true">
      <g stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" strokeLinejoin="round">
        <path className="botanical-ink botanical-stem" pathLength="1" d="M120 288C106 230 107 166 141 62M120 288C116 214 83 162 57 109M119 288C130 233 156 186 196 163M115 219C93 225 79 205 76 194C101 192 114 201 115 219M127 218C142 194 162 196 173 191C170 210 148 222 127 218M112 163C92 153 99 128 107 119C119 134 124 149 112 163M130 116C144 94 160 100 168 91C168 112 151 126 130 116" />
        {[[141, 60, 1], [57, 106, .85], [198, 159, .72], [92, 176, .5], [157, 213, .43]].map(([x, y, scale], i) => <g key={i} className={`botanical-bloom bloom-${i}`} transform={`translate(${x} ${y}) scale(${scale})`}>
          {[0, 72, 144, 216, 288].map(angle => <path className="botanical-ink botanical-petal" key={angle} pathLength="1" transform={`rotate(${angle})`} d="M0-4C-18-6-22-24-12-29C-4-32 3-24 1-15C9-27 21-25 20-16C19-7 9-3 0-4Z" />)}
          <circle className="botanical-center" r="3.2" />
        </g>)}
      </g>
    </svg>
  );
}
export function Bird({ small = false }: { small?: boolean }) {
  return (
    <svg
      className={small ? "bird small" : "bird"}
      viewBox="0 0 400 240"
      fill="none"
      aria-hidden="true"
    >
      <g
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path
          className="draw"
          pathLength="1"
          d="M58 199C133 198 198 188 343 209M116 199C147 182 153 161 155 145M144 177C118 186 111 166 106 155C127 150 138 158 144 177M156 156C172 153 187 139 188 126C166 123 151 139 156 156"
        />
        <path
          className="draw"
          pathLength="1"
          d="M166 150C152 122 159 81 196 76C229 72 246 93 244 118C258 126 280 138 296 153C264 156 243 156 220 146C204 163 183 168 166 150ZM199 86C179 88 168 98 166 114M170 124C198 102 208 106 220 127C208 140 192 148 175 145M246 98L265 105L245 110M190 162L184 191M202 157L205 192M177 192H193M199 193H213"
        />
        <circle cx="231" cy="97" r="2.1" fill="currentColor" />
        <path className="bird-detail" d="M174 118L201 117M180 125L205 121M188 131L208 127M223 144L270 150M231 140L279 151M235 134L283 145M206 79C203 66 208 57 219 53M213 67L228 63" />
        <path
          className="bird-orbit"
          opacity=".4"
          strokeDasharray="3 7"
          d="M51 100C92 29 234 29 314 75C389 118 343 170 306 177"
        />
        <path d="M327 71L336 79M336 71L327 79M96 53V64M91 58H102" />
        <g className="bird-detail" transform="translate(63 109) scale(1.35)" strokeWidth="0.9">
          <path d={birdShapes[2]} /><circle cx="24" cy="16" r="1" fill="currentColor" />
        </g>
        <g className="bird-detail" transform="translate(281 41) scale(1.2)" strokeWidth="1">
          <path d={birdShapes[1]} /><circle cx="24" cy="16" r="1" fill="currentColor" />
        </g>
      </g>
      <text className="bird-note"
        x="285"
        y="196"
        fill="currentColor"
        fontSize="13"
        fontFamily="monospace"
        transform="rotate(-6 285 196)"
      >
        room to grow.
      </text>
    </svg>
  );
}
export function AgentGlyph({ index = 0 }: { index?: number }) {
  return (
    <svg
      className="agent-glyph"
      viewBox="0 0 40 40"
      fill="none"
      aria-hidden="true"
    >
      <path
        d={birdShapes[((index % 4) + 4) % 4]}
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="24" cy="16" r="1" fill="currentColor" />
    </svg>
  );
}

export function BirdPair() {
  return (
    <svg className="bird-pair" viewBox="0 0 180 116" fill="none" aria-hidden="true">
      <g stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round">
        <path className="draw" pathLength="1" d="M15 97C56 91 106 92 164 98M52 94C60 83 59 70 57 62M56 81C44 82 36 73 38 66C51 65 57 71 56 81M59 72C68 70 77 60 73 54C62 56 57 63 59 72" />
        <g transform="translate(38 16) scale(1.8)"><path className="draw" pathLength="1" d={birdShapes[0]} /><circle cx="24" cy="16" r="1" fill="currentColor" /></g>
        <g transform="translate(153 43) scale(-1.25 1.25)"><path className="draw" pathLength="1" d={birdShapes[3]} /><circle cx="24" cy="16" r="1" fill="currentColor" /></g>
        <path opacity=".45" d="M27 30L32 35M32 30L27 35M139 18V26M135 22H143" />
      </g>
    </svg>
  );
}
