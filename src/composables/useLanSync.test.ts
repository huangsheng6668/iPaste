import { describe, expect, it, vi } from "vitest";
import { createPinia } from "pinia";
import { createApp, defineComponent } from "vue";
import type { App } from "vue";
import { useLanSync } from "./useLanSync";

/**
 * 真实 composable 测试：通过 createApp 挂载宿主组件，让 useLanSync 的
 * setup/onMounted/onUnmounted 生命周期真实执行（不用 @vue/test-utils，零新依赖）。
 *
 * 宿主策略说明：本项目 vitest 跑在默认 node 环境（vite.config.ts 无 test.environment，
 * devDependencies 也无 jsdom/happy-dom），因此用下方 ~30 行的最小 DOM 桩补齐 Vue
 * mount 路径与 i18n/pinia 初始化真正会触到的全局表面：
 * - window/Element/SVGElement：runtime-dom normalizeContainer / mount 包装的 instanceof 守卫；
 * - document.createElement/createComment：容器节点与「render 返回 null」的注释占位节点；
 * - localStorage + document.documentElement：i18n 模块初始化与 store setup 里的 setLanguage。
 */

interface FakeNode {
  nodeType: number;
  parentNode: FakeNode | null;
  childNodes: FakeNode[];
  textContent: string;
  lang: string;
  insertBefore(child: FakeNode, anchor: FakeNode | null): void;
  removeChild(child: FakeNode): void;
}

// vi.hoisted 会被提升到所有 import 之前：i18n.ts 在模块顶层就访问 localStorage，
// DOM 桩必须先于 useLanSync 的依赖链（→ ipasteStore → i18n）求值时安装好。
vi.hoisted(() => {
  const fakeNode = (nodeType: number): FakeNode => {
    const node: FakeNode = {
      nodeType,
      parentNode: null,
      childNodes: [],
      textContent: "",
      lang: "",
      insertBefore(child, _anchor) {
        child.parentNode = node;
        node.childNodes.push(child);
      },
      removeChild(child) {
        node.childNodes = node.childNodes.filter((current) => current !== child);
        child.parentNode = null;
      },
    };
    return node;
  };
  const globals = globalThis as unknown as Record<string, unknown>;
  if (!globals.window) globals.window = globalThis;
  if (!globals.Element) globals.Element = class Element {};
  if (!globals.SVGElement) globals.SVGElement = class SVGElement {};
  if (!globals.localStorage) {
    globals.localStorage = { getItem: () => null, setItem: () => {} };
  }
  if (!globals.document) {
    globals.document = {
      createElement: () => fakeNode(1),
      createComment: () => fakeNode(8),
      createTextNode: () => fakeNode(3),
      documentElement: fakeNode(1),
    };
  }
});

const mocks = vi.hoisted(() => {
  const idle = {
    role: null,
    status: "idle" as const,
    code: null,
    listenAddr: null,
    peerDeviceName: null,
  };
  return {
    lanGetState: vi.fn(async () => idle),
    lanCreateSession: vi.fn(),
    lanDisconnect: vi.fn(async () => undefined),
    listen: vi.fn(async () => () => {}),
  };
});

vi.mock("../lib/env", () => ({ isTauri: true }));
vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    lanGetState: mocks.lanGetState,
    lanCreateSession: mocks.lanCreateSession,
    lanDisconnect: mocks.lanDisconnect,
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

/** 挂载最小宿主组件并把 composable 返回值捕获到外层，断言后由调用方 unmount。 */
function mountLanSync(): { lan: ReturnType<typeof useLanSync>; app: App } {
  let lan!: ReturnType<typeof useLanSync>;
  const host = defineComponent({
    setup() {
      lan = useLanSync();
      return () => null;
    },
  });
  const app = createApp(host);
  app.use(createPinia());
  app.mount(document.createElement("div"));
  return { lan, app };
}

describe("useLanSync createSession 错误映射", () => {
  it("port_in_use 拒绝映射为 portConflict{name,pid}，error 保持 null", async () => {
    mocks.lanCreateSession.mockRejectedValueOnce({
      code: "port_in_use",
      message: "端口 45130 被 a.exe（PID 5）占用。",
      params: { port: 45130, name: "a.exe", pid: 5 },
    });
    const { lan, app } = mountLanSync();

    await lan.createSession();

    expect(lan.portConflict.value).toEqual({ name: "a.exe", pid: 5 });
    expect(lan.error.value).toBeNull();
    // 宿主挂载让 onMounted 真实执行：等全部 8 个 lan 事件监听注册完成。
    await vi.waitFor(() => expect(mocks.listen.mock.calls).toHaveLength(8));

    app.unmount();
  });

  it("普通字符串拒绝走 error 文案，portConflict 保持 null", async () => {
    mocks.lanCreateSession.mockRejectedValueOnce("network unreachable");
    const { lan, app } = mountLanSync();

    await lan.createSession();

    expect(lan.error.value).toBe("network unreachable");
    expect(lan.portConflict.value).toBeNull();

    app.unmount();
  });
});
