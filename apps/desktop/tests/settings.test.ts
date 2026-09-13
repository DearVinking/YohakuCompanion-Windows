import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";

import { api } from "../src/api";
import GeneralView from "../src/views/GeneralView.vue";
import { deferred, emptyPatch, initialSettings } from "./fixtures";

const wrappers: VueWrapper[] = [];
const unhandledErrors: unknown[] = [];

beforeEach(async () => {
    unhandledErrors.length = 0;
    await api.updateSettings(structuredClone(initialSettings));
});

afterEach(() => {
    wrappers.splice(0).forEach((wrapper) => wrapper.unmount());
    vi.restoreAllMocks();
});

async function mountGeneral() {
    const wrapper = mount(GeneralView, {
        global: { config: { errorHandler: (error) => unhandledErrors.push(error) } },
    });
    wrappers.push(wrapper);
    await flushPromises();
    return wrapper;
}

describe("settings patch semantics", () => {
    it("ignores nullable fields while applying explicit false and true values", async () => {
        await api.updateSettings({
            ...emptyPatch,
            shareApplications: false,
            shareWindowTitles: true,
        });

        expect(await api.getSettings()).toEqual({
            ...initialSettings,
            shareApplications: false,
            shareWindowTitles: true,
        });
    });

    it("keeps every setting when all patch fields are null", async () => {
        await api.updateSettings(emptyPatch);
        expect(await api.getSettings()).toEqual(initialSettings);
    });

    it("normalizes player names to match Rust settings", async () => {
        await api.updateSettings({
            ...emptyPatch,
            preferredPlayers: ["  Spotify ", " ", "QQMusic"],
        });
        expect((await api.getSettings()).preferredPlayers).toEqual(["spotify", "qqmusic"]);
    });

    it("uses Rust Unicode whitespace when normalizing player names", async () => {
        await api.updateSettings({
            ...emptyPatch,
            preferredPlayers: ["\u0085Spotify\u0085", "\uFEFFQQMusic\uFEFF"],
        });
        expect((await api.getSettings()).preferredPlayers).toEqual(["spotify", "\uFEFFqqmusic\uFEFF"]);
    });

    it.each([{ players: [] }, { players: [" ", "\t"] }])("keeps preferred players for an empty normalized list $players", async ({ players }) => {
        await api.updateSettings({ ...emptyPatch, preferredPlayers: players });
        expect((await api.getSettings()).preferredPlayers).toEqual(["spotify", "cloudmusic", "qqmusic"]);
    });
});

describe("general settings inputs", () => {
    it("sends a complete nullable patch for a toggle", async () => {
        const save = vi.spyOn(api, "updateSettings");
        const wrapper = await mountGeneral();

        await wrapper.get('button[role="switch"]').trigger("click");
        await flushPromises();

        expect(save).toHaveBeenCalledWith({
            shareApplications: false,
            shareWindowTitles: null,
            shareMedia: null,
            ignoreNullArtist: null,
            launchAtLogin: null,
            pauseSharing: null,
            preferredPlayers: null,
        });
        expect(wrapper.get('button[role="switch"]').attributes("aria-checked")).toBe("false");
    });

    it("saves players only on native change using a complete nullable patch", async () => {
        const save = vi.spyOn(api, "updateSettings");
        const wrapper = await mountGeneral();
        const input = wrapper.get("input");

        input.element.value = " VLC,  Spotify, ";
        await input.trigger("input");
        expect(save).not.toHaveBeenCalled();
        await input.trigger("change");
        await flushPromises();

        expect(save).toHaveBeenCalledWith({
            shareApplications: null,
            shareWindowTitles: null,
            shareMedia: null,
            ignoreNullArtist: null,
            launchAtLogin: null,
            pauseSharing: null,
            preferredPlayers: ["VLC", "Spotify"],
        });
        expect(input.element.value).toBe("vlc, spotify");
    });

    it("reports a failed initial load and lets the user retry", async () => {
        vi.spyOn(api, "getSettings").mockRejectedValueOnce({ code: "STORE" });
        const wrapper = await mountGeneral();

        expect(wrapper.find('[role="alert"]').exists()).toBe(true);
        expect(wrapper.get('[role="alert"]').text()).toContain("无法读取或保存本机数据");
        await wrapper.get("button").trigger("click");
        await flushPromises();

        expect(wrapper.find('[role="alert"]').exists()).toBe(false);
        expect(wrapper.findAll('[role="switch"]')).toHaveLength(4);
        expect(unhandledErrors).toEqual([]);
    });

    it("reports a failed save, restores the confirmed toggle, and permits retry", async () => {
        vi.spyOn(api, "updateSettings").mockRejectedValueOnce({ code: "STORE" });
        const wrapper = await mountGeneral();
        const toggle = wrapper.get('[role="switch"]');

        await toggle.trigger("click");
        await flushPromises();
        expect(wrapper.find('[role="alert"]').exists()).toBe(true);
        expect(toggle.attributes("aria-checked")).toBe("true");

        await toggle.trigger("click");
        await flushPromises();
        expect(wrapper.find('[role="alert"]').exists()).toBe(false);
        expect((await api.getSettings()).shareApplications).toBe(false);
        expect(unhandledErrors).toEqual([]);
    });

    it("preserves two rapid changes to the same toggle in save order", async () => {
        const gate = deferred<void>();
        const update = api.updateSettings;
        const save = vi.spyOn(api, "updateSettings").mockImplementationOnce(async (patch) => {
            await gate.promise;
            return update(patch);
        });
        const wrapper = await mountGeneral();
        const toggle = wrapper.get('[role="switch"]');

        await toggle.trigger("click");
        await toggle.trigger("click");
        const writesWhilePending = save.mock.calls.length;
        gate.resolve();
        await flushPromises();

        expect(writesWhilePending).toBe(1);
        expect((await api.getSettings()).shareApplications).toBe(true);
        expect(toggle.attributes("aria-checked")).toBe("true");
    });

    it("keeps a later field edit when an earlier response is delayed", async () => {
        const gate = deferred<void>();
        const update = api.updateSettings;
        vi.spyOn(api, "updateSettings").mockImplementationOnce(async (patch) => {
            const saved = await update(patch);
            await gate.promise;
            return saved;
        });
        const wrapper = await mountGeneral();
        const toggles = wrapper.findAll('[role="switch"]');

        await toggles[0].trigger("click");
        await flushPromises();
        await toggles[1].trigger("click");
        gate.resolve();
        await flushPromises();

        expect((await api.getSettings()).shareApplications).toBe(false);
        expect((await api.getSettings()).shareWindowTitles).toBe(true);
        expect(toggles[0].attributes("aria-checked")).toBe("false");
        expect(toggles[1].attributes("aria-checked")).toBe("true");
    });

    it("loads the accepted edits when the page is remounted during a save", async () => {
        const gate = deferred<void>();
        const update = api.updateSettings;
        vi.spyOn(api, "updateSettings").mockImplementationOnce(async (patch) => {
            await gate.promise;
            return update(patch);
        });
        const first = await mountGeneral();
        const toggles = first.findAll('[role="switch"]');
        await toggles[0].trigger("click");
        await toggles[1].trigger("click");
        first.unmount();
        wrappers.splice(wrappers.indexOf(first), 1);

        const next = await mountGeneral();
        gate.resolve();
        await flushPromises();

        const loaded = next.findAll('[role="switch"]');
        expect(loaded[0].attributes("aria-checked")).toBe("false");
        expect(loaded[1].attributes("aria-checked")).toBe("true");
    });

    it("backfills only the latest repeated player change", async () => {
        await api.updateSettings({ ...emptyPatch, preferredPlayers: ["spotify"] });
        const gate = deferred<void>();
        const update = api.updateSettings;
        const save = vi.spyOn(api, "updateSettings").mockImplementationOnce(async (patch) => {
            await gate.promise;
            return update(patch);
        });
        const wrapper = await mountGeneral();
        const input = wrapper.get("input");

        await input.setValue("");
        await input.setValue("VLC");
        await input.setValue("");
        gate.resolve();
        await flushPromises();

        expect(save).toHaveBeenCalledTimes(3);
        expect((await api.getSettings()).preferredPlayers).toEqual(["vlc"]);
        expect(input.element.value).toBe("vlc");
    });

    it("preserves repeated player input that has not emitted another change", async () => {
        await api.updateSettings({ ...emptyPatch, preferredPlayers: ["spotify"] });
        const gate = deferred<void>();
        const update = api.updateSettings;
        const save = vi.spyOn(api, "updateSettings").mockImplementationOnce(async (patch) => {
            await gate.promise;
            return update(patch);
        });
        const wrapper = await mountGeneral();
        const input = wrapper.get("input");

        await input.setValue("");
        input.element.value = "VLC";
        await input.trigger("input");
        input.element.value = "";
        await input.trigger("input");
        gate.resolve();
        await flushPromises();

        expect(save).toHaveBeenCalledTimes(1);
        expect((await api.getSettings()).preferredPlayers).toEqual(["spotify"]);
        expect(input.element.value).toBe("");
    });

    it("does not overwrite player text entered while an earlier change is saving", async () => {
        const gate = deferred<void>();
        const update = api.updateSettings;
        vi.spyOn(api, "updateSettings").mockImplementationOnce(async (patch) => {
            const saved = await update(patch);
            await gate.promise;
            return saved;
        });
        const wrapper = await mountGeneral();
        const input = wrapper.get("input");

        await input.setValue("VLC");
        await flushPromises();
        input.element.value = "Next Player";
        await input.trigger("input");
        gate.resolve();
        await flushPromises();

        expect(input.element.value).toBe("Next Player");
        expect((await api.getSettings()).preferredPlayers).toEqual(["vlc"]);
        await input.trigger("change");
        await flushPromises();
        expect((await api.getSettings()).preferredPlayers).toEqual(["next player"]);
    });
});
