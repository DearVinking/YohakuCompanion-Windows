import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";

import { api } from "../src/api";
import SegmentedControl from "../src/design-system/components/SegmentedControl.vue";
import PrivacyView from "../src/views/PrivacyView.vue";
import { deferred, initialRules } from "./fixtures";

const wrappers: VueWrapper[] = [];
const unhandledErrors: unknown[] = [];

beforeEach(async () => {
    unhandledErrors.length = 0;
    await api.updatePrivacyRules(structuredClone(initialRules));
});

afterEach(() => {
    wrappers.splice(0).forEach((wrapper) => wrapper.unmount());
    vi.restoreAllMocks();
});

async function mountPrivacy() {
    const wrapper = mount(PrivacyView, {
        global: { config: { errorHandler: (error) => unhandledErrors.push(error) } },
    });
    wrappers.push(wrapper);
    await flushPromises();
    return wrapper;
}

describe("privacy rule editing", () => {
    it.each([
        { field: "application", group: 3, level: "hide" },
        { field: "windowTitle", group: 4, level: "share" },
        { field: "media", group: 5, level: "inherit" },
    ] as const)("saves the $field level on an existing application rule", async ({ field, group, level }) => {
        const wrapper = await mountPrivacy();
        const control = wrapper.findAllComponents(SegmentedControl)[group];
        await control.get(`input[value="${level}"]`).setValue(true);
        await flushPromises();

        expect((await api.getPrivacyRules()).apps["editor.exe"][field]).toBe(level);
        expect(control.get<HTMLInputElement>(`input[value="${level}"]`).element.checked).toBe(true);
    });

    it("saves aliases on native change only, preserving spaces and empty-to-null", async () => {
        const save = vi.spyOn(api, "updatePrivacyRules");
        const wrapper = await mountPrivacy();
        const alias = wrapper.get<HTMLInputElement>('input[aria-label="自定义应用名称"]');

        alias.element.value = "  Custom name  ";
        await alias.trigger("input");
        expect(save).not.toHaveBeenCalled();
        expect((await api.getPrivacyRules()).apps["editor.exe"].displayAlias).toBe("Editor");
        await alias.trigger("change");
        await flushPromises();
        expect((await api.getPrivacyRules()).apps["editor.exe"].displayAlias).toBe("  Custom name  ");

        await alias.setValue("");
        await flushPromises();
        expect((await api.getPrivacyRules()).apps["editor.exe"].displayAlias).toBeNull();
        expect(alias.element.value).toBe("");
    });

    it("displays the rule returned by the save", async () => {
        const result = structuredClone(initialRules);
        result.apps["editor.exe"].displayAlias = "Saved name";
        vi.spyOn(api, "updatePrivacyRules").mockResolvedValueOnce(result);
        const wrapper = await mountPrivacy();
        const alias = wrapper.get<HTMLInputElement>('input[aria-label="自定义应用名称"]');

        await alias.setValue("New name");
        await flushPromises();
        expect(alias.element.value).toBe("Saved name");
    });

    it.each(["__proto__", "constructor"])("adds and removes %s as an ordinary application key", async (key) => {
        const wrapper = await mountPrivacy();
        await wrapper.get('input[aria-label="应用程序文件名"]').setValue(key);
        await wrapper.findAll("button").find((button) => button.text() === "添加规则")!.trigger("click");
        await flushPromises();

        const saved = await api.getPrivacyRules();
        expect(Object.hasOwn(saved.apps, key)).toBe(true);
        expect(saved.apps[key]).toEqual({
            application: "inherit",
            windowTitle: "inherit",
            media: "inherit",
            displayAlias: null,
        });
        expect(Object.getPrototypeOf(saved.apps)).toBe(Object.prototype);

        const deleteButtons = wrapper.findAll("button").filter((button) => button.text() === "删除规则");
        expect(deleteButtons).toHaveLength(2);
        await deleteButtons[1].trigger("click");
        await flushPromises();
        expect(Object.hasOwn((await api.getPrivacyRules()).apps, key)).toBe(false);
    });

    it.each(["loaded", "new"])("updates %s special-key selection before the pending save returns", async (origin) => {
        if (origin === "loaded") {
            await api.updatePrivacyRules({
                ...structuredClone(initialRules),
                apps: {
                    ...initialRules.apps,
                    ["__proto__"]: {
                        application: "inherit",
                        windowTitle: "inherit",
                        media: "inherit",
                        displayAlias: null,
                    },
                },
            });
        }
        const wrapper = await mountPrivacy();
        const gate = deferred<void>();
        const update = api.updatePrivacyRules;
        vi.spyOn(api, "updatePrivacyRules").mockImplementationOnce(async (rules) => {
            await gate.promise;
            return update(rules);
        });
        if (origin === "new") {
            await wrapper.get('input[aria-label="应用程序文件名"]').setValue("__proto__");
            await wrapper.findAll("button").find((button) => button.text() === "添加规则")!.trigger("click");
        }

        const control = wrapper.findAllComponents(SegmentedControl)[6];
        await control.get('input[value="hide"]').setValue(true);
        const selectedWhileSaving = control.get<HTMLInputElement>('[data-segment-selected="true"] input').element.value;
        gate.resolve();
        await flushPromises();

        expect(selectedWhileSaving).toBe("hide");
        const saved = await api.getPrivacyRules();
        expect(Object.hasOwn(saved.apps, "__proto__")).toBe(true);
        expect(saved.apps["__proto__"]).toEqual({
            application: "hide",
            windowTitle: "inherit",
            media: "inherit",
            displayAlias: null,
        });
    });

    it("captures each save before later edits and preserves the latest rule", async () => {
        const gate = deferred<void>();
        const update = api.updatePrivacyRules;
        const save = vi.spyOn(api, "updatePrivacyRules").mockImplementationOnce(async (rules) => {
            const saved = await update(rules);
            await gate.promise;
            return saved;
        });
        const wrapper = await mountPrivacy();
        const controls = wrapper.findAllComponents(SegmentedControl);

        await controls[3].get('input[value="hide"]').setValue(true);
        await flushPromises();
        await controls[5].get('input[value="hide"]').setValue(true);
        const firstSubmittedMedia = save.mock.calls[0][0].apps["editor.exe"].media;
        const writesWhilePending = save.mock.calls.length;
        gate.resolve();
        await flushPromises();

        expect(firstSubmittedMedia).toBe("share");
        expect(writesWhilePending).toBe(1);
        expect((await api.getPrivacyRules()).apps["editor.exe"]).toMatchObject({ application: "hide", media: "hide" });
        expect(controls[5].get<HTMLInputElement>('input[value="hide"]').element.checked).toBe(true);
    });

    it("preserves an alias being typed when an earlier alias save returns", async () => {
        const gate = deferred<void>();
        const update = api.updatePrivacyRules;
        vi.spyOn(api, "updatePrivacyRules").mockImplementationOnce(async (rules) => {
            const saved = await update(rules);
            await gate.promise;
            return saved;
        });
        const wrapper = await mountPrivacy();
        const alias = wrapper.get<HTMLInputElement>('input[aria-label="自定义应用名称"]');

        await alias.setValue("Saved alias");
        await flushPromises();
        alias.element.value = "Still typing";
        await alias.trigger("input");
        gate.resolve();
        await flushPromises();

        expect(alias.element.value).toBe("Still typing");
        expect((await api.getPrivacyRules()).apps["editor.exe"].displayAlias).toBe("Saved alias");
        await alias.trigger("change");
        await flushPromises();
        expect((await api.getPrivacyRules()).apps["editor.exe"].displayAlias).toBe("Still typing");
    });

    it("loads the accepted rules when the page is remounted during a save", async () => {
        const gate = deferred<void>();
        const update = api.updatePrivacyRules;
        vi.spyOn(api, "updatePrivacyRules").mockImplementationOnce(async (rules) => {
            await gate.promise;
            return update(rules);
        });
        const first = await mountPrivacy();
        const controls = first.findAllComponents(SegmentedControl);
        await controls[3].get('input[value="hide"]').setValue(true);
        await controls[5].get('input[value="hide"]').setValue(true);
        first.unmount();
        wrappers.splice(wrappers.indexOf(first), 1);

        const next = await mountPrivacy();
        gate.resolve();
        await flushPromises();

        const loaded = next.findAllComponents(SegmentedControl);
        expect(loaded[3].get<HTMLInputElement>('input[value="hide"]').element.checked).toBe(true);
        expect(loaded[5].get<HTMLInputElement>('input[value="hide"]').element.checked).toBe(true);
    });

    it("reports a failed initial load and permits retry", async () => {
        vi.spyOn(api, "getPrivacyRules").mockRejectedValueOnce({ code: "STORE" });
        const wrapper = await mountPrivacy();

        expect(wrapper.find('[role="alert"]').exists()).toBe(true);
        await wrapper.get("button").trigger("click");
        await flushPromises();
        expect(wrapper.find('[role="alert"]').exists()).toBe(false);
        expect(wrapper.text()).toContain("editor.exe");
        expect(unhandledErrors).toEqual([]);
    });

    it("keeps failed edits for the next save and clears the error after retry", async () => {
        vi.spyOn(api, "updatePrivacyRules").mockRejectedValueOnce({ code: "STORE" });
        const wrapper = await mountPrivacy();
        const controls = wrapper.findAllComponents(SegmentedControl);

        await controls[3].get('input[value="hide"]').setValue(true);
        await flushPromises();
        expect(wrapper.find('[role="alert"]').exists()).toBe(true);
        expect((await api.getPrivacyRules()).apps["editor.exe"].application).toBe("inherit");

        await controls[5].get('input[value="hide"]').setValue(true);
        await flushPromises();
        expect(wrapper.find('[role="alert"]').exists()).toBe(false);
        expect((await api.getPrivacyRules()).apps["editor.exe"]).toMatchObject({ application: "hide", media: "hide" });
        expect(unhandledErrors).toEqual([]);
    });
});
