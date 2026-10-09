import { useState } from "react";
import type { PublicData } from "../lib/api";
import { isRecord } from "../lib/api";
type SeriesPoint = { date: string; spend: number; repayment: number };
const isSeriesPoint = (value: unknown): value is SeriesPoint => isRecord(value) && typeof value.date === "string" && typeof value.spend === "number" && Number.isFinite(value.spend) && typeof value.repayment === "number" && Number.isFinite(value.repayment);
export function Chart({ data }: { data: PublicData }) {
  const [period, setPeriod] = useState(30);
  const [hover, setHover] = useState<number | null>(null);
  const series = data.series.filter(isSeriesPoint).slice(-period);
  const max = Math.max(
    150,
    ...series.flatMap((s) => [Number(s.spend), Number(s.repayment)]),
  );
  const points = (key: "spend" | "repayment") =>
    series
      .map(
        (s, i) =>
          `${50 + (i * 650) / Math.max(1, series.length - 1)},${200 - (Number(s[key]) / max) * 160}`,
      )
      .join(" ");
  const current = hover === null ? null : series[hover];
  return (
    <section className="panel chart-panel">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">spend and repayment</span>
          <h2>credit activity</h2>
        </div>
        <div className="segments">
          {[7, 30].map((n) => (
            <button
              key={n}
              className={period === n ? "selected" : ""}
              onClick={() => {
                setPeriod(n);
                setHover(null);
              }}
            >
              {n}d
            </button>
          ))}
        </div>
      </div>
      <div className="chart-meta">
        <div className="legend">
          <span>
            <i className="blue-dot" />
            provider spend
          </span>
          <span>
            <i className="mint-dot" />
            repayments
          </span>
        </div>
        <span className="mono">
          {current
            ? `${current.date}: ${Number(current.spend).toFixed(2)} / ${Number(current.repayment).toFixed(2)} USDT`
            : "USDT / day"}
        </span>
      </div>
      <div className="chart-wrap">
        <svg
          viewBox="0 0 740 244"
          role="img"
          aria-label={`${period} day ${data.mode} daily provider spend and repayments chart`}
          onMouseLeave={() => setHover(null)}
        >
          {[0, 50, 100, 150].map((v) => (
            <g key={v}>
              <line
                x1="50"
                x2="710"
                y1={200 - (v / max) * 160}
                y2={200 - (v / max) * 160}
                stroke="var(--line)"
                strokeDasharray="2 5"
              />
              <text
                x="6"
                y={205 - (v / max) * 160}
                fill="var(--muted)"
                fontSize="10"
              >
                {v}
              </text>
            </g>
          ))}
          {series.length > 0 ? (
            <>
              <defs>
                <linearGradient id="chart-fill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0" stopColor="#a9d5ed" stopOpacity=".12" />
                  <stop offset="1" stopColor="#a9d5ed" stopOpacity="0" />
                </linearGradient>
              </defs>
              <polygon
                points={`50,200 ${points("spend")} 700,200`}
                fill="url(#chart-fill)"
              />
              <polyline
                points={points("spend")}
                stroke="var(--blue)"
                strokeWidth="2"
                fill="none"
                strokeLinejoin="round"
              />
              <polyline
                points={points("repayment")}
                stroke="var(--mint)"
                strokeWidth="1.5"
                fill="none"
                strokeDasharray="5 5"
              />
              {series.map((s, i) => (
                <g key={i}>
                  <rect
                    x={40 + (i * 650) / Math.max(1, series.length - 1)}
                    y="25"
                    width={650 / series.length + 8}
                    height="178"
                    fill="transparent"
                    onMouseEnter={() => setHover(i)}
                  />
                  {(i % Math.max(1, Math.floor(series.length / 5)) === 0 ||
                    i === series.length - 1) && (
                    <text
                      x={50 + (i * 650) / Math.max(1, series.length - 1)}
                      y="232"
                      fill="var(--muted)"
                      fontSize="10"
                      textAnchor="middle"
                    >
                      {s.date}
                    </text>
                  )}
                </g>
              ))}
              {hover !== null && (
                <line
                  x1={50 + (hover * 650) / Math.max(1, series.length - 1)}
                  x2={50 + (hover * 650) / Math.max(1, series.length - 1)}
                  y1="25"
                  y2="200"
                  stroke="var(--muted)"
                  strokeDasharray="2 3"
                />
              )}
            </>
          ) : (
            <text
              x="370"
              y="110"
              textAnchor="middle"
              fill="var(--muted)"
              fontSize="14"
            >
              no settled activity yet
            </text>
          )}
        </svg>
      </div>
      <div className="chart-foot">
        <span>
          {data.mode === "example"
            ? "illustrative daily activity · not onchain receipts"
            : "receipts appear after provider settlement is connected"}
        </span>
        <span className="mono">spend → repay → review</span>
      </div>
    </section>
  );
}
