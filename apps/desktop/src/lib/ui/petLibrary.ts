import type { PetScene } from "./petScene";
import type { PetPreferences } from "./petPreferences";

export type PetDefaults = { scale: number; behavior: PetPreferences };
export type PetRecord = {
  id: string;
  identity: PetScene["pet"];
  defaults: PetDefaults;
  builtin: boolean;
};

export function filterPets(pets: PetRecord[], query: string, source: string) {
  const needle = query.trim().toLocaleLowerCase();
  return pets.filter(
    (pet) =>
      (!needle ||
        (pet.identity.displayName ?? "")
          .toLocaleLowerCase()
          .includes(needle)) &&
      (source === "all" || (source === "builtin" ? pet.builtin : !pet.builtin)),
  );
}

export function scenesForPet(scenes: PetScene[], petId: string) {
  return scenes
    .filter((scene) => scene.pet.petId === petId)
    .sort((a, b) => Number(b.favorite) - Number(a.favorite));
}
