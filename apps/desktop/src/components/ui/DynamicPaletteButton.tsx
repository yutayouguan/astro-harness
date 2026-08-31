import { useId, useRef } from "react";

type Props = {
  label: string;
  onReshuffle: () => void;
};

/** 灵动色彩模式在主聊天页的轻量重配色入口。 */
export default function DynamicPaletteButton({ label, onReshuffle }: Props) {
  const rotorRef = useRef<SVGGElement | null>(null);
  const gradientPrefix = `dynamic-pinwheel-${useId().replace(/:/g, "")}`;
  const gradientId = (name: string) => `${gradientPrefix}-${name}`;

  const reshuffle = () => {
    const rotor = rotorRef.current;
    if (rotor) {
      const reduceMotion =
        typeof window !== "undefined" &&
        window.matchMedia("(prefers-reduced-motion: reduce)").matches;

      rotor.getAnimations().forEach((animation) => animation.cancel());
      rotor.animate(
        [
          { transform: "rotate(0deg)" },
          { transform: `rotate(${reduceMotion ? 360 : 720}deg)` },
        ],
        {
          duration: reduceMotion ? 1320 : 1680,
          easing: "cubic-bezier(0.77, 0, 0.175, 1)",
        },
      );
    }
    onReshuffle();
  };

  return (
    <button
      type="button"
      className="shell-dynamic-palette-button"
      onClick={reshuffle}
      title={label}
      aria-label={label}
    >
      <span className="shell-dynamic-palette-icon" aria-hidden>
        <svg viewBox="0 0 48 66" role="presentation">
          <defs>
            <linearGradient
              id={gradientId("blue")}
              x1="15.824"
              x2="13.221"
              y1="42.583"
              y2="13.276"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#23a6ed" />
              <stop offset=".269" stopColor="#1ea1e9" />
              <stop offset=".606" stopColor="#1191de" />
              <stop offset=".896" stopColor="#007ed1" />
            </linearGradient>
            <linearGradient
              id={gradientId("green")}
              x1="31.431"
              x2="25.755"
              y1="44.619"
              y2="23.159"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#57cc6d" />
              <stop offset="1" stopColor="#2d874b" />
            </linearGradient>
            <linearGradient
              id={gradientId("green-detail")}
              x1="19.403"
              x2="24.624"
              y1="24.691"
              y2="43.366"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#70e67a" />
              <stop offset=".592" stopColor="#55c46a" />
              <stop offset="1" stopColor="#1e7f49" />
            </linearGradient>
            <linearGradient
              id={gradientId("cyan")}
              x1="41.532"
              x2="29.966"
              y1="4.369"
              y2="26.745"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#51e6fb" />
              <stop offset=".571" stopColor="#2ad7e7" />
              <stop offset="1" stopColor="#00c7d1" />
            </linearGradient>
            <linearGradient
              id={gradientId("cyan-detail")}
              x1="30.833"
              x2="31.303"
              y1="22.679"
              y2="34.44"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#51e6fb" />
              <stop offset="1" stopColor="#00c7d1" />
            </linearGradient>
            <linearGradient
              id={gradientId("red")}
              x1="15.313"
              x2="23.258"
              y1=".588"
              y2="26.383"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#f06060" />
              <stop offset=".623" stopColor="#da3e2d" />
              <stop offset="1" stopColor="#cf2c13" />
            </linearGradient>
            <linearGradient
              id={gradientId("red-detail")}
              x1="26.995"
              x2="28.079"
              y1="6.041"
              y2="25.345"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#ff6c54" />
              <stop offset="1" stopColor="#f55239" />
            </linearGradient>
            <linearGradient
              id={gradientId("blue-detail")}
              x1="17.654"
              x2="16.101"
              y1="26.118"
              y2="18.986"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#a4e1f4" />
              <stop offset="1" stopColor="#44c2f9" />
            </linearGradient>
            <linearGradient
              id={gradientId("hub")}
              x1="23.833"
              x2="24.146"
              y1="22.933"
              y2="24.935"
              gradientUnits="userSpaceOnUse"
            >
              <stop offset="0" stopColor="#626363" />
              <stop offset="1" stopColor="#454546" />
            </linearGradient>
          </defs>
          <path className="shell-dynamic-palette-stem" d="M24 25v39.5" />
          <g ref={rotorRef} className="shell-dynamic-palette-rotor">
            <path
              data-blade="blue"
              fill={`url(#${gradientId("blue")})`}
              d="M6.579 41.856 23.815 24.188a.324.324 0 0 0 .039-.407c-1.048-1.464-2.982-1.717-3.699-2.701-2.123-2.913-2.662-4.561-3.574-4.459a.29.29 0 0 0-.165.087l-6.227 6.383C6.607 26.763 6.093 34.847 5.999 41.617c-.005.306.366.458.58.239Z"
            />
            <path
              data-blade="green"
              fill={`url(#${gradientId("green")})`}
              d="M41.855 41.42 24.187 24.183a.324.324 0 0 0-.407-.039c-1.464 1.049-1.717 2.982-2.701 3.699-2.913 2.123-4.561 2.662-4.459 3.574a.29.29 0 0 0 .087.165l6.383 6.227c3.672 3.582 11.756 4.096 18.526 4.19.306.005.458-.366.239-.579Z"
            />
            <path
              fill={`url(#${gradientId("green-detail")})`}
              fillRule="evenodd"
              d="M23.09 37.81c-2.006-1.957-2.936-5.827.909-13.81-6.249 2.793-8.439 6.464-7.362 7.515l6.453 6.295Z"
              clipRule="evenodd"
            />
            <path
              data-blade="cyan"
              fill={`url(#${gradientId("cyan")})`}
              d="M41.418 6.144 24.182 23.812a.324.324 0 0 0-.039.407c1.048 1.464 2.982 1.717 3.699 2.701 2.123 2.913 2.662 4.561 3.574 4.459a.29.29 0 0 0 .165-.087l6.226-6.383c3.582-3.672 4.096-11.756 4.19-18.526.006-.306-.365-.458-.579-.239Z"
            />
            <path
              fill={`url(#${gradientId("cyan-detail")})`}
              fillRule="evenodd"
              d="M37.809 24.909c-1.957 2.006-5.828 2.936-13.81-.909 2.793 6.249 6.464 8.439 7.515 7.362l6.295-6.453Z"
              clipRule="evenodd"
            />
            <path
              data-blade="red"
              fill={`url(#${gradientId("red")})`}
              d="m6.102 6.581 17.668 17.236a.324.324 0 0 0 .407.039c1.464-1.048 1.717-2.982 2.7-3.699 2.913-2.123 4.561-2.662 4.459-3.574a.29.29 0 0 0-.087-.165l-6.383-6.227C21.195 6.608 13.111 6.094 6.341 6c-.306-.004-.458.367-.239.581Z"
            />
            <path
              fill={`url(#${gradientId("red-detail")})`}
              fillRule="evenodd"
              d="M24.908 10.19c2.006 1.957 2.936 5.828-.909 13.81 6.249-2.793 8.439-6.464 7.362-7.515l-6.453-6.295Z"
              clipRule="evenodd"
            />
            <path
              fill={`url(#${gradientId("blue-detail")})`}
              fillRule="evenodd"
              d="M10.189 23.091c1.957-2.006 5.828-2.936 13.81.909-2.793-6.249-6.464-8.439-7.515-7.362l-6.295 6.453Z"
              clipRule="evenodd"
            />
            <circle
              cx="24"
              cy="24"
              r=".997"
              fill={`url(#${gradientId("hub")})`}
            />
          </g>
        </svg>
      </span>
    </button>
  );
}
