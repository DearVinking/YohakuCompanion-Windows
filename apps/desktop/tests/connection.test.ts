import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount, type VueWrapper } from "@vue/test-utils";

import App from "../src/App.vue";
import ConnectionView from "../src/views/ConnectionView.vue";
import { api } from "../src/api";
import type { PreviewView, Settings } from "../src/api/types";

const wrappers: VueWrapper[] = [];

beforeEach(async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    vi.setSystemTime(new Date("2026-09-13T08:00:00Z"));
    await api.removePairing();
    await api.setPaused(false);
});

afterEach(() => {
    wrappers.splice(0).forEach((wrapper) => wrapper.unmount());
    vi.restoreAllMocks();
    vi.useRealTimers();
});

async function pair() {
    await api.startPairing("https://core.example.com", "pair-code", "测试电脑");
}

async function mountApp() {
    const wrapper = mount(App, { global: { directives: { scrollbar: {} } } });
    wrappers.push(wrapper);
    await flushPromises();
    return wrapper;
}

async function mountConnection() {
    const wrapper = mount(ConnectionView);
    wrappers.push(wrapper);
    await flushPromises();
    return wrapper;
}

describe("右上角同步按钮", () => {
    it("配对后可直接开启、暂停并恢复同步", async () => {
        await pair();
        const wrapper = await mountApp();

        const inactiveButton = wrapper.get('header button[aria-label="未开启同步"]');
        expect(inactiveButton.attributes("aria-pressed")).toBe("false");
        expect(inactiveButton.find("svg.lucide-radio-off").exists()).toBe(true);
        await inactiveButton.trigger("click");
        await flushPromises();
        expect((await api.getConnectionStatus()).liveDeskEnabled).toBe(true);
        expect((await api.getSettings()).pauseSharing).toBe(false);

        const activeButton = wrapper.get('header button[aria-label="同步中"]');
        expect(activeButton.attributes("aria-pressed")).toBe("true");
        expect(activeButton.find("svg.lucide-radio").exists()).toBe(true);
        await activeButton.trigger("click");
        await flushPromises();
        expect((await api.getSettings()).pauseSharing).toBe(true);
        expect((await api.getConnectionStatus()).coordinator.state).toBe("suspended");

        const pausedButton = wrapper.get('header button[aria-label="已暂停同步"]');
        expect(pausedButton.attributes("aria-pressed")).toBe("false");
        expect(pausedButton.find("svg.lucide-radio-off").exists()).toBe(true);
        await pausedButton.trigger("click");
        await flushPromises();
        expect((await api.getSettings()).pauseSharing).toBe(false);
        expect((await api.getConnectionStatus()).coordinator.state).toBe("active");
    });

    it("未配对时禁用同步，完成配对后立即可开启", async () => {
        const wrapper = await mountApp();
        expect(wrapper.get('header button[aria-label="未配对"]').attributes("disabled")).toBeDefined();

        await wrapper.get('input[placeholder="粘贴配对码"]').setValue("pair-code");
        const pairButton = wrapper.findAll("main button").find((button) => button.text() === "连接")!;
        await pairButton.trigger("click");
        await flushPromises();

        expect(wrapper.get('header button[aria-label="未开启同步"]').attributes("disabled")).toBeUndefined();
        expect((await api.getConnectionStatus()).liveDeskEnabled).toBe(false);
    });

    it("之前暂停过也能一次点击开启新配对的同步", async () => {
        await pair();
        await api.setPaused(true);
        const wrapper = await mountApp();

        await wrapper.get('header button[aria-label="未开启同步"]').trigger("click");
        await flushPromises();

        expect((await api.getConnectionStatus()).liveDeskEnabled).toBe(true);
        expect((await api.getSettings()).pauseSharing).toBe(false);
    });

    it("无需成功读取预览即可从其他页面开启同步", async () => {
        await pair();
        vi.spyOn(api, "getPreview").mockRejectedValue({ code: "INTERNAL" });
        const wrapper = await mountApp();
        const generalButton = wrapper.findAll("nav button").find((button) => button.text() === "通用")!;
        await generalButton.trigger("click");
        await flushPromises();

        await wrapper.get('header button[aria-label="未开启同步"]').trigger("click");
        await flushPromises();
        expect((await api.getConnectionStatus()).liveDeskEnabled).toBe(true);
    });

    it("开启失败时显示错误，并允许重试", async () => {
        await pair();
        vi.spyOn(api, "enableLiveDesk").mockRejectedValueOnce({ code: "STORE" });
        const wrapper = await mountApp();

        await wrapper.get('header button[aria-label="未开启同步"]').trigger("click");
        await flushPromises();
        expect(wrapper.get('header [role="alert"]').text()).toContain("无法读取或保存本机数据");

        await wrapper.get('header button[aria-label="未开启同步"]').trigger("click");
        await flushPromises();
        expect(wrapper.find('header [role="alert"]').exists()).toBe(false);
        expect((await api.getConnectionStatus()).liveDeskEnabled).toBe(true);
    });

    it("正在开启时禁用按钮，避免重复操作", async () => {
        await pair();
        const enable = api.enableLiveDesk;
        let finish!: () => void;
        const pending = new Promise<void>((resolve) => { finish = resolve; });
        const enableSpy = vi.spyOn(api, "enableLiveDesk").mockImplementation(async () => {
            await pending;
            await enable();
        });
        const wrapper = await mountApp();

        await wrapper.get('header button[aria-label="未开启同步"]').trigger("click");
        await flushPromises();
        expect(wrapper.get('header button[aria-label="未开启同步"]').attributes("disabled")).toBeDefined();
        expect(enableSpy).toHaveBeenCalledTimes(1);

        finish();
        await flushPromises();
        expect(wrapper.get('header button[aria-label="同步中"]').attributes("disabled")).toBeUndefined();
    });

    it("迟到的轮询不能覆盖暂停后的按钮状态", async () => {
        await pair();
        await api.enableLiveDesk();
        const wrapper = await mountApp();
        const generalButton = wrapper.findAll("nav button").find((button) => button.text() === "通用")!;
        await generalButton.trigger("click");
        await flushPromises();

        const oldSettings = await api.getSettings();
        let finish!: (settings: Settings) => void;
        vi.spyOn(api, "getSettings").mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();

        await wrapper.get('header button[aria-label="同步中"]').trigger("click");
        await flushPromises();
        expect(wrapper.find('header button[aria-label="已暂停同步"]').exists()).toBe(true);
        finish(oldSettings);
        await flushPromises();
        expect(wrapper.find('header button[aria-label="已暂停同步"]').exists()).toBe(true);

        await wrapper.get('header button[aria-label="已暂停同步"]').trigger("click");
        await flushPromises();
        expect((await api.getSettings()).pauseSharing).toBe(false);
    });

    it("状态读取恢复后清除临时错误", async () => {
        await pair();
        const wrapper = await mountApp();
        const generalButton = wrapper.findAll("nav button").find((button) => button.text() === "通用")!;
        await generalButton.trigger("click");
        await flushPromises();
        vi.spyOn(api, "getConnectionStatus").mockRejectedValueOnce({ code: "STORE" });

        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();
        expect(wrapper.find('header [role="alert"]').exists()).toBe(true);

        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();
        expect(wrapper.find('header [role="alert"]').exists()).toBe(false);
    });
});

describe("同步内容", () => {
    it("配对后自动读取并持续更新内容，在播放内容下显示最近同步时间", async () => {
        await pair();
        const first = await api.getPreview();
        const next: PreviewView = {
            ...first,
            application: { displayName: "Visual Studio Code", windowTitle: null },
            media: null,
        };
        vi.spyOn(api, "getPreview").mockResolvedValueOnce(first).mockResolvedValue(next);
        const status = await api.getConnectionStatus();
        const lastSentAt = "2026-09-13T07:59:00Z";
        vi.spyOn(api, "getConnectionStatus").mockResolvedValue({
            ...status,
            coordinator: { ...status.coordinator, lastSentAt },
        });
        const wrapper = await mountConnection();
        const content = wrapper.findAll("section").find((section) => section.find("h2").text() === "同步内容")!;

        expect(content.text()).toContain("Microsoft Edge");
        expect(content.text()).toContain("示例歌曲");
        expect(content.findAll("button")).toHaveLength(0);
        const separator = content.get("hr");
        expect(separator.element.previousElementSibling?.textContent).toContain("播放内容");
        expect(separator.element.nextElementSibling?.textContent).toContain("最近同步时间");
        expect(separator.element.nextElementSibling?.textContent).toContain(new Date(lastSentAt).toLocaleString());
        expect(wrapper.findAll("section")[0].text()).not.toContain("最近同步");

        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();
        expect(content.text()).toContain("Visual Studio Code");
        expect(content.text()).toContain("播放内容：不显示");
        expect(content.text()).not.toContain("示例歌曲");
    });

    it("尚未同步时显示空状态，读取失败后不会继续展示过期内容", async () => {
        await pair();
        const wrapper = await mountConnection();
        expect(wrapper.text()).toContain("尚未同步");
        expect(wrapper.text()).toContain("Microsoft Edge");
        vi.spyOn(api, "getPreview").mockRejectedValueOnce({ code: "INTERNAL" });

        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();
        expect(wrapper.text()).not.toContain("Microsoft Edge");
        expect(wrapper.text()).toContain("操作未完成，请稍后重试");

        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();
        expect(wrapper.text()).toContain("Microsoft Edge");
    });

    it("解除配对后移除同步内容并禁用右上角按钮", async () => {
        await pair();
        const wrapper = await mountApp();
        const unpairButton = wrapper.findAll("button").find((button) => button.text() === "解除配对")!;
        await unpairButton.trigger("click");
        await flushPromises();

        expect(wrapper.text()).not.toContain("Microsoft Edge");
        expect(wrapper.text()).not.toContain("同步内容");
        expect(wrapper.get('header button[aria-label="未配对"]').attributes("disabled")).toBeDefined();
    });

    it("解除配对会立即移除卡片，迟到的内容刷新不能恢复旧卡片", async () => {
        await pair();
        const wrapper = await mountApp();
        const oldPreview = await api.getPreview();
        let finish!: (preview: PreviewView) => void;
        vi.spyOn(api, "getPreview").mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
        await vi.advanceTimersByTimeAsync(1000);
        await flushPromises();

        const unpairButton = wrapper.findAll("main button").find((button) => button.text() === "解除配对")!;
        await unpairButton.trigger("click");
        await flushPromises();
        try {
            expect(wrapper.text()).not.toContain("同步内容");
        } finally {
            finish(oldPreview);
            await flushPromises();
        }
        expect(wrapper.text()).not.toContain("Microsoft Edge");
        expect(wrapper.text()).not.toContain("同步内容");
    });
});
