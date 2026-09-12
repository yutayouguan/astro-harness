import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DesktopPetCanvas from "../components/desktop-pet/DesktopPetCanvas";
import naitang from "../assets/pets/naitang/apng/motion-clips.json";
import pudding from "../assets/pets/pudding/apng/motion-clips.json";
import type { PetMotionClips } from "../lib/ui/petMotionClip";

const files = import.meta.glob("../assets/pets/*/apng/*.apng", {
  eager: true,
  query: "?url",
  import: "default",
});
function Preview() {
  const [pet, setPet] = useState("naitang");
  const [action, setAction] = useState("idle");
  const [reduced, setReduced] = useState(false);
  const source = pet === "naitang" ? naitang : pudding;
  const clips: PetMotionClips = Object.fromEntries(
    Object.entries(source).map(([name, spec]) => [
      name,
      {
        ...spec,
        path: files[`../assets/pets/${pet}/apng/${spec.path}`] as string,
      },
    ]),
  );
  return (
    <div
      style={{
        padding: 24,
        background: "white",
        color: "#222",
        minHeight: 420,
      }}
    >
      <label>
        宠物
        <select
          aria-label="APNG宠物"
          value={pet}
          onChange={(e) => {
            setPet(e.target.value);
            setAction("idle");
          }}
        >
          <option value="naitang">奶糖</option>
          <option value="pudding">布丁</option>
        </select>
      </label>
      <label>
        动作
        <select
          aria-label="APNG动作"
          value={action}
          onChange={(e) => setAction(e.target.value)}
        >
          {Object.keys(clips).map((name) => (
            <option key={name}>{name}</option>
          ))}
        </select>
      </label>
      <label>
        <input
          type="checkbox"
          checked={reduced}
          onChange={(e) => setReduced(e.target.checked)}
        />
        减少动态
      </label>
      <div style={{ display: "flex", gap: 24, marginTop: 20 }}>
        {["#fff", "#171923"].map((background) => (
          <div key={background} style={{ background, width: 240, height: 260 }}>
            <DesktopPetCanvas
              src={clips.idle.path}
              motionClips={clips}
              state="idle"
              motionName={action}
              repeatMotion
              reducedMotion={reduced}
              label={`${pet} APNG ${background}`}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

export default { title: "Desktop/PetApng", component: Preview } satisfies Meta<
  typeof Preview
>;
export const Actions: StoryObj<typeof Preview> = {};
