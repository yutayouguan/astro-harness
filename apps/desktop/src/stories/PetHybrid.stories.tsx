import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DesktopPetCanvas from "../components/desktop-pet/DesktopPetCanvas";
import naitang from "../assets/pets/naitang/apng/motion-clips.json";
import pudding from "../assets/pets/pudding/apng/motion-clips.json";
const files = import.meta.glob("../assets/pets/*/apng/*.apng", {
  eager: true,
  query: "?url",
  import: "default",
});
const atlases = import.meta.glob("../assets/pets/*/spritesheet.webp", {
  eager: true,
  query: "?url",
  import: "default",
});

function Preview() {
  const [pet, setPet] = useState("naitang"),
    [action, setAction] = useState("idle");
  const [angle, setAngle] = useState<number | null>(null),
    [paused, setPaused] = useState(false),
    [reduced, setReduced] = useState(false);
  const [unmatched, setUnmatched] = useState(false);
  const [legacy, setLegacy] = useState(false);
  const clips = Object.fromEntries(
    Object.entries(pet === "naitang" ? naitang : pudding).map(
      ([name, clip]) => [
        name,
        {
          ...clip,
          path: files[`../assets/pets/${pet}/apng/${clip.path}`] as string,
        },
      ],
    ),
  );
  return (
    <div style={{ color: "#20232a", background: "#ddd", padding: 20 }}>
      <select
        aria-label="宠物"
        value={pet}
        onChange={(e) => {
          setPet(e.target.value);
          setAction("idle");
        }}
      >
        <option value="naitang">奶糖</option>
        <option value="pudding">布丁</option>
      </select>
      <select
        aria-label="动作"
        value={action}
        onChange={(e) => setAction(e.target.value)}
      >
        {Object.keys(clips).map((name) => (
          <option key={name}>{name}</option>
        ))}
      </select>
      <button onClick={() => setAngle(90)}>看右侧</button>
      <button onClick={() => setAngle(270)}>看左侧</button>
      <button onClick={() => setAngle(null)}>回正</button>
      <label>
        <input
          type="checkbox"
          checked={paused}
          onChange={(e) => setPaused(e.target.checked)}
        />
        暂停
      </label>
      <label>
        <input
          type="checkbox"
          checked={reduced}
          onChange={(e) => setReduced(e.target.checked)}
        />
        减少动态
      </label>
      <label>
        <input
          type="checkbox"
          checked={unmatched}
          onChange={(e) => setUnmatched(e.target.checked)}
        />
        不匹配素材
      </label>
      <label>
        <input
          type="checkbox"
          checked={legacy}
          onChange={(e) => setLegacy(e.target.checked)}
        />
        旧版待机图集
      </label>
      <div style={{ display: "flex", gap: 16, marginTop: 20 }}>
        {["white", "#171923"].map((background) => (
          <div key={background} style={{ width: 300, height: 280, background }}>
            <DesktopPetCanvas
              src={
                legacy
                  ? (atlases[
                      `../assets/pets/${pet}/spritesheet.webp`
                    ] as string)
                  : unmatched
                    ? clips.look.path
                    : clips.idle.path
              }
              motionClips={
                legacy
                  ? undefined
                  : unmatched
                    ? { ...clips, idle: clips.look }
                    : clips
              }
              state={angle == null ? "idle" : "look"}
              motionName={action === "idle" || legacy ? undefined : action}
              lookAngle={angle}
              paused={paused}
              reducedMotion={reduced}
              repeatMotion
              label={pet}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
export default {
  title: "Desktop/PetHybrid",
  component: Preview,
} satisfies Meta<typeof Preview>;
export const Layers: StoryObj<typeof Preview> = {};
