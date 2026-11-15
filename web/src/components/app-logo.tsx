import { cn } from "@/lib/utils";

interface AppLogoProps {
  className?: string;
  /** 多个实例同页时给渐变 id 加后缀，避免 SVG defs 冲突。 */
  uid?: string;
}

/** 应用图标：立方体 + 下载箭头，与桌面图标 / favicon 同一套视觉。 */
export function AppLogo({ className, uid = "a" }: AppLogoProps) {
  const bg = "lycodl-bg-" + uid;
  const stroke = "lycodl-stroke-" + uid;
  const aura = "lycodl-aura-" + uid;
  return (
    <svg
      viewBox="0 0 512 512"
      role="img"
      aria-label="lycodl"
      className={cn("size-9", className)}
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id={bg} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#111c2e" />
          <stop offset="0.5" stopColor="#0b1322" />
          <stop offset="1" stopColor="#050a12" />
        </linearGradient>
        <linearGradient id={stroke} x1="0.1" y1="0" x2="0.9" y2="1">
          <stop offset="0" stopColor="#7ff0ff" />
          <stop offset="0.45" stopColor="#22d3ee" />
          <stop offset="1" stopColor="#7c86ff" />
        </linearGradient>
        <radialGradient id={aura} cx="0.5" cy="0.46" r="0.52">
          <stop offset="0" stopColor="#22d3ee" stopOpacity="0.34" />
          <stop offset="1" stopColor="#22d3ee" stopOpacity="0" />
        </radialGradient>
      </defs>
      <rect x="16" y="16" width="480" height="480" rx="112" fill={"url(#" + bg + ")"} />
      <rect
        x="16"
        y="16"
        width="480"
        height="480"
        rx="112"
        fill="none"
        stroke="#7ff0ff"
        strokeOpacity="0.16"
        strokeWidth="6"
      />
      <circle cx="256" cy="238" r="196" fill={"url(#" + aura + ")"} />
      <g
        fill="none"
        stroke={"url(#" + stroke + ")"}
        strokeWidth="17"
        strokeLinejoin="round"
        strokeLinecap="round"
      >
        <path d="M256 96 L394 178 L256 260 L118 178 Z" opacity="0.95" />
        <path d="M118 178 V306 L256 388 L394 306 V178" opacity="0.42" />
      </g>
      <path d="M256 132 L318 168 L256 204 L194 168 Z" fill={"url(#" + stroke + ")"} opacity="0.5" />
      <path d="M234 232 h44 v52 h46 l-68 74 -68-74 h46 z" fill={"url(#" + stroke + ")"} />
    </svg>
  );
}
