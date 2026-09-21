import type {CSSProperties, ReactNode} from "react";
import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";

const colors = {
  background: "#F5F0E8",
  paper: "#FFFDF8",
  ink: "#17212B",
  muted: "#69717A",
  border: "#D9D2C7",
  upstream: "#7B838C",
  upstreamSoft: "#E8E8E5",
  orange: "#D85B2A",
  orangeDark: "#A83C17",
  orangeSoft: "#F8DCCF",
};

const font =
  'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
const mono = '"SFMono-Regular", Consolas, "Liberation Mono", monospace';

type PanelProps = {
  eyebrow: string;
  title: string;
  accent: string;
  children: ReactNode;
};

const Panel = ({eyebrow, title, accent, children}: PanelProps) => (
  <div
    style={{
      width: 538,
      height: 374,
      borderRadius: 22,
      border: `1px solid ${colors.border}`,
      background: colors.paper,
      boxShadow: "0 18px 44px rgba(23, 33, 43, 0.08)",
      overflow: "hidden",
      position: "relative",
    }}
  >
    <div style={{height: 5, background: accent}} />
    <div style={{padding: "21px 26px 0"}}>
      <div
        style={{
          color: accent,
          fontSize: 14,
          lineHeight: 1,
          fontWeight: 800,
          letterSpacing: 1.8,
          textTransform: "uppercase",
        }}
      >
        {eyebrow}
      </div>
      <div
        style={{
          color: colors.ink,
          fontSize: 27,
          lineHeight: 1.15,
          fontWeight: 760,
          marginTop: 9,
          letterSpacing: -0.7,
        }}
      >
        {title}
      </div>
    </div>
    {children}
  </div>
);

type PillProps = {
  children: ReactNode;
  background: string;
  color: string;
  style?: CSSProperties;
};

const Pill = ({children, background, color, style}: PillProps) => (
  <div
    style={{
      display: "inline-flex",
      alignItems: "center",
      borderRadius: 999,
      background,
      color,
      fontFamily: mono,
      fontSize: 13,
      lineHeight: 1,
      fontWeight: 700,
      padding: "8px 11px",
      ...style,
    }}
  >
    {children}
  </div>
);

const incoming = [
  {time: "09:00", value: 100, frame: 30},
  {time: "09:01", value: 101, frame: 50},
  {time: "09:19", value: 104, frame: 70},
  {time: "09:20", value: 103, frame: 90},
  {time: "09:20", value: 105, frame: 122, revision: true},
  {time: "09:45", value: 110, frame: 174},
] as const;

const dotEnter = (frame: number, arrival: number, fps: number) =>
  spring({frame: frame - arrival, fps, config: {damping: 15, stiffness: 170}});

const EventStrip = () => {
  const frame = useCurrentFrame();
  const active = [...incoming].reverse().find((item) => frame >= item.frame);
  const label =
    frame < 114
      ? "irregular observations arrive"
      : frame < 158
        ? "09:20 is revised"
        : frame < 198
          ? "the clock jumps to 09:45"
          : "same input · different window";

  return (
    <div
      style={{
        height: 42,
        display: "flex",
        justifyContent: "center",
        alignItems: "center",
        gap: 12,
        marginTop: 15,
      }}
    >
      <div
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: colors.orange,
        }}
      />
      <div
        style={{
          color: colors.ink,
          fontFamily: mono,
          fontSize: 15,
          fontWeight: 700,
          letterSpacing: -0.2,
        }}
      >
        {active ? `${active.time}  ${active.value}` : "waiting for data"}
      </div>
      <div style={{width: 1, height: 17, background: colors.border}} />
      <div style={{color: colors.muted, fontSize: 15, fontWeight: 650}}>{label}</div>
    </div>
  );
};

const UpstreamPanel = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const visible = incoming.filter((item) => frame >= item.frame);
  const firstKept = Math.max(0, visible.length - 3);

  return (
    <Panel eyebrow="upstream ta" title="SMA(3 observations)" accent={colors.upstream}>
      <div style={{position: "absolute", inset: "105px 26px 22px"}}>
        <div
          style={{
            color: colors.muted,
            fontSize: 14,
            fontWeight: 650,
            marginBottom: 19,
          }}
        >
          Every <span style={{fontFamily: mono}}>next()</span> call advances the window
        </div>
        <div
          style={{
            height: 144,
            borderRadius: 16,
            background: "#F6F5F2",
            border: `1px solid ${colors.border}`,
            padding: "18px 16px 13px",
            display: "flex",
            gap: 8,
            alignItems: "stretch",
          }}
        >
          {incoming.map((item, index) => {
            const entered = dotEnter(frame, item.frame, fps);
            const isVisible = frame >= item.frame;
            const isKept = isVisible && index >= firstKept && index < visible.length;
            const isExpired = isVisible && index < firstKept;
            return (
              <div
                key={`${item.time}-${item.value}`}
                style={{
                  width: 72,
                  opacity: isVisible ? (isExpired ? 0.32 : 1) : 0.12,
                  transform: `translateY(${(1 - entered) * 10}px) scale(${0.92 + entered * 0.08})`,
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  justifyContent: "flex-end",
                  transition: "none",
                }}
              >
                <div
                  style={{
                    color: isKept ? colors.ink : colors.muted,
                    fontFamily: mono,
                    fontWeight: 800,
                    fontSize: 17,
                    marginBottom: 8,
                    textDecoration: isExpired ? "line-through" : "none",
                  }}
                >
                  {item.value}
                </div>
                <div
                  style={{
                    width: 15,
                    height: 15,
                    borderRadius: "50%",
                    background: isKept ? colors.upstream : "#BBBDBD",
                    boxShadow: isKept ? "0 0 0 5px rgba(123, 131, 140, 0.15)" : "none",
                  }}
                />
                <div
                  style={{
                    height: 32,
                    width: 2,
                    background: isKept ? colors.upstream : "#CCCECC",
                  }}
                />
                <div
                  style={{
                    color: colors.muted,
                    fontFamily: mono,
                    fontSize: 11,
                    whiteSpace: "nowrap",
                  }}
                >
                  {item.time}
                </div>
                {"revision" in item && item.revision ? (
                  <div style={{color: colors.upstream, fontSize: 10, fontWeight: 800, marginTop: 2}}>
                    revision
                  </div>
                ) : (
                  <div style={{height: 12}} />
                )}
              </div>
            );
          })}
        </div>
        <div style={{display: "flex", alignItems: "center", gap: 10, marginTop: 17}}>
          <Pill background={colors.upstreamSoft} color={colors.ink}>
            keep last 3 calls
          </Pill>
          {frame >= 122 && frame < 174 ? (
            <span style={{color: colors.muted, fontSize: 13, fontWeight: 650}}>
              revision = another observation
            </span>
          ) : null}
        </div>
      </div>
    </Panel>
  );
};

const minuteX = (minute: number) => 30 + (minute / 45) * 425;
const valueY = (value: number) => 117 - (value - 100) * 6.2;

const ChronoPanel = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const jump = interpolate(frame, [166, 194], [20, 45], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.inOut(Easing.cubic),
  });
  const nowMinute = frame < 166 ? 20 : jump;
  const startMinute = Math.max(0, nowMinute - 30);
  const startX = minuteX(startMinute);
  const endX = minuteX(nowMinute);
  const revision = interpolate(frame, [122, 144], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.inOut(Easing.cubic),
  });
  const points = [
    {minute: 0, value: 100, frame: 30, label: "09:00"},
    {minute: 1, value: 101, frame: 50, label: "09:01"},
    {minute: 19, value: 104, frame: 70, label: "09:19"},
    {minute: 20, value: 103 + revision * 2, frame: 90, label: "09:20"},
    {minute: 45, value: 110, frame: 174, label: "09:45"},
  ];

  return (
    <Panel eyebrow="chrono-ta" title="SMA(30 minutes)" accent={colors.orange}>
      <div style={{position: "absolute", inset: "105px 26px 22px"}}>
        <div style={{color: colors.muted, fontSize: 14, fontWeight: 650, marginBottom: 19}}>
          Timestamps decide what belongs in the window
        </div>
        <div
          style={{
            height: 144,
            borderRadius: 16,
            background: "#FCF5F0",
            border: `1px solid ${colors.border}`,
            position: "relative",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              position: "absolute",
              left: startX,
              top: 12,
              width: Math.max(8, endX - startX),
              height: 106,
              borderRadius: 12,
              background: colors.orangeSoft,
              border: `1px solid rgba(216, 91, 42, 0.28)`,
            }}
          />
          <div
            style={{
              position: "absolute",
              left: startX + 8,
              top: 19,
              color: colors.orangeDark,
              fontFamily: mono,
              fontSize: 10,
              fontWeight: 800,
            }}
          >
            30 MIN
          </div>
          <div
            style={{
              position: "absolute",
              left: 30,
              right: 27,
              top: 118,
              height: 2,
              background: "#BFB9B0",
            }}
          />
          {[0, 15, 30, 45].map((minute) => (
            <div key={minute}>
              <div
                style={{
                  position: "absolute",
                  left: minuteX(minute),
                  top: 114,
                  width: 2,
                  height: 10,
                  background: "#BFB9B0",
                }}
              />
              <div
                style={{
                  position: "absolute",
                  left: minuteX(minute) - 14,
                  top: 126,
                  width: 30,
                  textAlign: "center",
                  color: colors.muted,
                  fontFamily: mono,
                  fontSize: 10,
                }}
              >
                {minute}
              </div>
            </div>
          ))}
          {points.map((point) => {
            const entered = dotEnter(frame, point.frame, fps);
            const retained = point.minute >= startMinute && point.minute <= nowMinute;
            return (
              <div
                key={point.label}
                style={{
                  position: "absolute",
                  left: minuteX(point.minute) - 8,
                  top: valueY(point.value) - 8,
                  width: 16,
                  height: 16,
                  borderRadius: "50%",
                  background: retained ? colors.orange : "#B8B1A9",
                  border: `3px solid ${colors.paper}`,
                  boxShadow: retained ? "0 0 0 5px rgba(216, 91, 42, 0.14)" : "none",
                  opacity: frame >= point.frame ? (retained ? 1 : 0.3) : 0,
                  transform: `scale(${entered})`,
                }}
              >
                <div
                  style={{
                    position: "absolute",
                    left: "50%",
                    bottom: point.minute === 1 || point.minute === 20 ? 32 : 17,
                    transform: "translateX(-50%)",
                    color: retained ? colors.ink : colors.muted,
                    fontFamily: mono,
                    fontSize: 11,
                    fontWeight: 800,
                    whiteSpace: "nowrap",
                    textDecoration: retained ? "none" : "line-through",
                    opacity: retained ? 1 : 0,
                  }}
                >
                  {Math.round(point.value)}
                </div>
              </div>
            );
          })}
        </div>
        <div style={{display: "flex", alignItems: "center", gap: 10, marginTop: 17}}>
          <Pill background={colors.orangeSoft} color={colors.orangeDark}>
            keep last 30 elapsed minutes
          </Pill>
          {frame >= 122 && frame < 174 ? (
            <span style={{color: colors.orangeDark, fontSize: 13, fontWeight: 800}}>
              same minute → replace
            </span>
          ) : null}
          {frame >= 194 ? (
            <span style={{color: colors.orangeDark, fontSize: 13, fontWeight: 800}}>
              expired by the clock
            </span>
          ) : null}
        </div>
      </div>
    </Panel>
  );
};

export const WindowSemantics = () => {
  const frame = useCurrentFrame();
  const conclusion = interpolate(frame, [194, 208], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill
      style={{
        background: colors.background,
        color: colors.ink,
        fontFamily: font,
        padding: "31px 48px 27px",
      }}
    >
      <div style={{display: "flex", justifyContent: "space-between", alignItems: "flex-start"}}>
        <div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              color: colors.orangeDark,
              fontSize: 14,
              fontWeight: 850,
              letterSpacing: 2,
              textTransform: "uppercase",
            }}
          >
            <span style={{fontFamily: mono}}>chrono-ta</span>
            <span style={{width: 28, height: 2, background: colors.orange}} />
            time-aware technical analysis
          </div>
          <div
            style={{
              marginTop: 7,
              fontSize: 34,
              fontWeight: 780,
              letterSpacing: -1.2,
              lineHeight: 1.08,
            }}
          >
            “Window” can mean two different things.
          </div>
        </div>
        <Pill background={colors.upstreamSoft} color={colors.muted} style={{marginTop: 7}}>
          same observations
        </Pill>
      </div>

      <EventStrip />

      <div style={{display: "flex", justifyContent: "space-between", gap: 28}}>
        <UpstreamPanel />
        <ChronoPanel />
      </div>

      <div
        style={{
          marginTop: 17,
          display: "flex",
          justifyContent: "center",
          alignItems: "center",
          gap: 10,
          opacity: 0.48 + conclusion * 0.52,
          transform: `translateY(${(1 - conclusion) * 4}px)`,
          color: colors.ink,
          fontSize: 18,
          fontWeight: 720,
          letterSpacing: -0.25,
        }}
      >
        <span>last N calls</span>
        <span style={{color: colors.muted, fontWeight: 500}}>vs.</span>
        <span style={{color: colors.orangeDark}}>the time that actually elapsed</span>
      </div>
    </AbsoluteFill>
  );
};
