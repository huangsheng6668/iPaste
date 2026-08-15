import { ref, watch } from "vue";
import type { useIpasteStore } from "../stores/ipasteStore";

type IpasteStore = ReturnType<typeof useIpasteStore>;

type ClipListScrollDeps = {
  store: IpasteStore;
};

/**
 * 剪贴列表滚动（原 App.vue clip-list scroll 段）：滚动条 780ms 淡出、
 * history 触底自动加载更多、搜索 160ms 防抖重载与选中卡片 rAF 滚动
 * （边缘留白 16px）。watch 由 App.vue 在 onMounted 经 setupWatches() 接线，
 * onUnmounted 调 cleanup() 清理全部定时器。
 */
export function useClipListScroll(deps: ClipListScrollDeps) {
  const { store } = deps;
  const clipListElement = ref<HTMLElement | null>(null);
  const isClipListScrolling = ref(false);
  let clipListScrollTimer: number | null = null;
  let selectionScrollFrame: number | null = null;
  let searchReloadTimer: number | null = null;

  function showClipListScrollbar() {
    clearClipListScrollTimer();
    isClipListScrolling.value = true;
    clipListScrollTimer = window.setTimeout(() => {
      isClipListScrolling.value = false;
      clipListScrollTimer = null;
    }, 780);
  }

  function handleClipListScroll() {
    showClipListScrollbar();

    const list = clipListElement.value;
    if (!list || store.selectedCategoryId !== "history" || !store.hasMoreClips) return;

    const distanceToBottom = list.scrollHeight - list.scrollTop - list.clientHeight;
    if (distanceToBottom < 160) {
      void store.loadMoreClips();
    }
  }

  function clearClipListScrollTimer() {
    if (clipListScrollTimer === null) return;
    window.clearTimeout(clipListScrollTimer);
    clipListScrollTimer = null;
  }

  function resetClipListScroll() {
    clearClipListScrollTimer();
    isClipListScrolling.value = false;

    if (clipListElement.value) {
      clipListElement.value.scrollTop = 0;
    }
  }

  function scheduleSelectedClipScroll() {
    clearSelectionScrollFrame();
    selectionScrollFrame = window.requestAnimationFrame(() => {
      selectionScrollFrame = null;
      scrollSelectedClipIntoView();
    });
  }

  function clearSelectionScrollFrame() {
    if (selectionScrollFrame === null) return;
    window.cancelAnimationFrame(selectionScrollFrame);
    selectionScrollFrame = null;
  }

  function scheduleSearchReload() {
    clearSearchReloadTimer();
    searchReloadTimer = window.setTimeout(() => {
      searchReloadTimer = null;
      void store.reloadClips();
    }, 160);
  }

  function clearSearchReloadTimer() {
    if (searchReloadTimer === null) return;
    window.clearTimeout(searchReloadTimer);
    searchReloadTimer = null;
  }

  function scrollSelectedClipIntoView() {
    const list = clipListElement.value;
    const selectedCard = list?.querySelector<HTMLElement>(".clip-card-selected");
    if (!list || !selectedCard) return;

    const listRect = list.getBoundingClientRect();
    const cardRect = selectedCard.getBoundingClientRect();
    const edgePadding = 16;
    const visibleTop = listRect.top + edgePadding;
    const visibleBottom = listRect.bottom - edgePadding;

    if (cardRect.top < visibleTop) {
      list.scrollBy({ top: cardRect.top - visibleTop, behavior: "auto" });
      showClipListScrollbar();
      return;
    }

    if (cardRect.bottom > visibleBottom) {
      list.scrollBy({ top: cardRect.bottom - visibleBottom, behavior: "auto" });
      showClipListScrollbar();
    }
  }

  function setupWatches() {
    watch(
      () => store.search,
      () => {
        store.clampSelection();
        if (store.selectedCategoryId === "history") {
          scheduleSearchReload();
        }
      },
    );

    watch(
      () => [store.selectedIndex, store.selectedCategoryId, store.search],
      () => scheduleSelectedClipScroll(),
      { flush: "post" },
    );
  }

  function cleanup() {
    clearClipListScrollTimer();
    clearSelectionScrollFrame();
    clearSearchReloadTimer();
  }

  return {
    clipListElement,
    isClipListScrolling,
    showClipListScrollbar,
    handleClipListScroll,
    resetClipListScroll,
    scheduleSelectedClipScroll,
    scheduleSearchReload,
    setupWatches,
    cleanup,
  };
}
