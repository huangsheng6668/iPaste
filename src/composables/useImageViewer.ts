import { computed, ref, nextTick } from "vue";

const IMAGE_FIT_PADDING = 0;
const IMAGE_MIN_SCALE = 0.05;
const IMAGE_MAX_SCALE = 8;
const IMAGE_ZOOM_STEP = 1.2;

type ViewerCallbacks = {
  clearImageTextSelection: () => void;
};

export function useImageViewer(callbacks: ViewerCallbacks) {
  const imageStageElement = ref<HTMLElement | null>(null);
  const imageNaturalSize = ref({ width: 0, height: 0 });
  const imageStageSize = ref({ width: 0, height: 0 });
  const imageScale = ref(1);
  const imageRotation = ref(0);
  const imagePan = ref({ x: 0, y: 0 });
  const imageViewMode = ref<"fit" | "actual" | "manual">("fit");
  const isImageDragging = ref(false);
  let imageDragState: {
    pointerId: number;
    startX: number;
    startY: number;
    panX: number;
    panY: number;
  } | null = null;

  const normalizedImageRotation = computed(() => ((imageRotation.value % 360) + 360) % 360);
  const isImageRotatedSideways = computed(() => normalizedImageRotation.value === 90 || normalizedImageRotation.value === 270);
  const rotatedImageBaseSize = computed(() => {
    const { width, height } = imageNaturalSize.value;
    if (!width || !height) return { width: 0, height: 0 };
    return isImageRotatedSideways.value ? { width: height, height: width } : { width, height };
  });
  const imageDisplaySize = computed(() => ({
    width: rotatedImageBaseSize.value.width * imageScale.value,
    height: rotatedImageBaseSize.value.height * imageScale.value,
  }));
  const imagePanBounds = computed(() => ({
    x: Math.max(0, (imageDisplaySize.value.width - imageStageSize.value.width) / 2),
    y: Math.max(0, (imageDisplaySize.value.height - imageStageSize.value.height) / 2),
  }));
  const canPanImage = computed(() => imagePanBounds.value.x > 0 || imagePanBounds.value.y > 0);
  const isImageActualSize = computed(() => Math.abs(imageScale.value - 1) < 0.005);
  const imageZoomLabel = computed(() => `${Math.round(imageScale.value * 100)}%`);
  const imageStyle = computed(() => {
    const { width, height } = imageNaturalSize.value;
    return {
      width: width ? `${width}px` : "auto",
      height: height ? `${height}px` : "auto",
      marginLeft: width ? `${-width / 2}px` : "0",
      marginTop: height ? `${-height / 2}px` : "0",
      transform: `rotate(${imageRotation.value}deg) scale(${imageScale.value})`,
    };
  });
  const imageFrameStyle = computed(() => ({
    transform: `translate(${imagePan.value.x}px, ${imagePan.value.y}px)`,
  }));
  const canZoomOutImage = computed(() => imageScale.value > minimumImageScale() + 0.005);
  const canZoomInImage = computed(() => imageScale.value < IMAGE_MAX_SCALE - 0.005);

  function updateImageStageSize() {
    const stage = imageStageElement.value;
    if (!stage) return;

    const rect = stage.getBoundingClientRect();
    imageStageSize.value = {
      width: rect.width,
      height: rect.height,
    };
  }

  function handleImageLoad(event: Event) {
    const target = event.currentTarget as HTMLImageElement;
    imageNaturalSize.value = {
      width: target.naturalWidth,
      height: target.naturalHeight,
    };
    void nextTick(() => {
      updateImageStageSize();
      fitImageToStage();
    });
  }

  function resetImageViewState() {
    imageNaturalSize.value = { width: 0, height: 0 };
    imageStageSize.value = { width: 0, height: 0 };
    imageScale.value = 1;
    imageRotation.value = 0;
    imagePan.value = { x: 0, y: 0 };
    imageViewMode.value = "fit";
    endImageDrag();
  }

  function fitImageToStage() {
    callbacks.clearImageTextSelection();
    imageScale.value = fitImageScale();
    imagePan.value = { x: 0, y: 0 };
    imageViewMode.value = "fit";
  }

  function showImageActualSize() {
    callbacks.clearImageTextSelection();
    updateImageStageSize();
    imageScale.value = 1;
    imagePan.value = { x: 0, y: 0 };
    imageViewMode.value = "actual";
    clampImagePan();
  }

  function zoomImageIn() {
    setImageScale(imageScale.value * IMAGE_ZOOM_STEP);
  }

  function zoomImageOut() {
    setImageScale(imageScale.value / IMAGE_ZOOM_STEP);
  }

  function setImageScale(nextScale: number, anchor?: { x: number; y: number }) {
    updateImageStageSize();
    const currentScale = imageScale.value;
    const clampedScale = clamp(nextScale, minimumImageScale(), IMAGE_MAX_SCALE);
    if (!Number.isFinite(clampedScale) || Math.abs(clampedScale - currentScale) < 0.001) return;
    callbacks.clearImageTextSelection();

    if (anchor) {
      const stage = imageStageElement.value;
      const rect = stage?.getBoundingClientRect();
      if (rect) {
        const anchorX = anchor.x - rect.left - rect.width / 2;
        const anchorY = anchor.y - rect.top - rect.height / 2;
        const ratio = clampedScale / currentScale;
        imagePan.value = {
          x: anchorX - (anchorX - imagePan.value.x) * ratio,
          y: anchorY - (anchorY - imagePan.value.y) * ratio,
        };
      }
    }

    imageScale.value = clampedScale;
    imageViewMode.value = isImageActualSize.value ? "actual" : "manual";
    clampImagePan();
  }

  function rotateImageClockwise() {
    callbacks.clearImageTextSelection();
    const shouldRefit = imageViewMode.value === "fit";
    imageRotation.value = normalizedImageRotation.value + 90;
    if (shouldRefit) {
      void nextTick(fitImageToStage);
      return;
    }

    void nextTick(clampImagePan);
  }

  function handleImageWheel(event: WheelEvent) {
    event.preventDefault();
    const direction = event.deltaY < 0 ? 1 : -1;
    const factor = direction > 0 ? IMAGE_ZOOM_STEP : 1 / IMAGE_ZOOM_STEP;
    setImageScale(imageScale.value * factor, { x: event.clientX, y: event.clientY });
  }

  function startImagePan(event: PointerEvent) {
    if (event.button !== 0) return;
    callbacks.clearImageTextSelection();
    if (!canPanImage.value) return;

    imageDragState = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      panX: imagePan.value.x,
      panY: imagePan.value.y,
    };
    isImageDragging.value = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function moveImagePan(event: PointerEvent) {
    if (!imageDragState || event.pointerId !== imageDragState.pointerId) return;

    imagePan.value = constrainImagePan({
      x: imageDragState.panX + event.clientX - imageDragState.startX,
      y: imageDragState.panY + event.clientY - imageDragState.startY,
    });
  }

  function finishImagePan(event: PointerEvent) {
    if (!imageDragState || event.pointerId !== imageDragState.pointerId) return;

    const target = event.currentTarget as HTMLElement;
    if (target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    endImageDrag();
  }

  function endImageDrag() {
    imageDragState = null;
    isImageDragging.value = false;
  }

  function clampImagePan() {
    imagePan.value = constrainImagePan(imagePan.value);
  }

  function constrainImagePan(nextPan: { x: number; y: number }) {
    const bounds = imagePanBounds.value;
    return {
      x: clamp(nextPan.x, -bounds.x, bounds.x),
      y: clamp(nextPan.y, -bounds.y, bounds.y),
    };
  }

  function fitImageScale() {
    const { width, height } = rotatedImageBaseSize.value;
    const stageWidth = imageStageSize.value.width;
    const stageHeight = imageStageSize.value.height;
    if (!width || !height || !stageWidth || !stageHeight) return 1;

    const availableWidth = Math.max(1, stageWidth - IMAGE_FIT_PADDING);
    const availableHeight = Math.max(1, stageHeight - IMAGE_FIT_PADDING);
    return clamp(Math.min(availableWidth / width, availableHeight / height, 1), IMAGE_MIN_SCALE, 1);
  }

  function minimumImageScale() {
    return Math.min(IMAGE_MIN_SCALE, fitImageScale());
  }

  function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
  }

  return {
    imageStageElement,
    imageNaturalSize,
    imageStageSize,
    imageScale,
    imageRotation,
    imagePan,
    imageViewMode,
    isImageDragging,
    normalizedImageRotation,
    isImageRotatedSideways,
    rotatedImageBaseSize,
    imageDisplaySize,
    imagePanBounds,
    canPanImage,
    isImageActualSize,
    imageZoomLabel,
    imageStyle,
    imageFrameStyle,
    canZoomOutImage,
    canZoomInImage,
    updateImageStageSize,
    handleImageLoad,
    resetImageViewState,
    fitImageToStage,
    showImageActualSize,
    zoomImageIn,
    zoomImageOut,
    setImageScale,
    rotateImageClockwise,
    handleImageWheel,
    startImagePan,
    moveImagePan,
    finishImagePan,
    endImageDrag,
    clampImagePan,
  };
}
