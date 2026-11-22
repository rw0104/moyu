export const MAX_IMAGE_BYTES = 32 * 1024 * 1024;
export const IMAGE_ACCEPT = 'image/png,image/jpeg,image/webp,image/gif,.png,.jpg,.jpeg,.webp,.gif';

export interface ImageAttachment {
  id: string;
  name: string;
  mimeType: string;
  dataUrl: string;
}

const supportedTypes = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif']);
const supportedExtension = /\.(png|jpe?g|webp|gif)$/i;
const extensionMime: Record<string, string> = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', webp: 'image/webp', gif: 'image/gif' };

/** Validation is repeated by the desktop backend before any request is sent. */
export async function readImageFile(file: File): Promise<ImageAttachment> {
  if (!file.size) throw new Error('这张图片是空文件，请重新选择。');
  if (file.size > MAX_IMAGE_BYTES) throw new Error('图片超过 32 MiB，请压缩后再试。');
  const declaredMime = file.type.toLowerCase();
  if (declaredMime ? !supportedTypes.has(declaredMime) : !supportedExtension.test(file.name)) {
    throw new Error('请选择 PNG、JPEG、WebP 或 GIF 图片。');
  }
  const mimeType = declaredMime || extensionMime[file.name.split('.').at(-1)!.toLowerCase()];
  const rawDataUrl = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error('图片读取失败，请重新选择。'));
    reader.onload = () => typeof reader.result === 'string' ? resolve(reader.result) : reject(new Error('图片读取失败，请重新选择。'));
    reader.readAsDataURL(file);
  });
  const dataUrl = rawDataUrl.replace(/^data:[^;]*;base64,/, `data:${mimeType};base64,`);
  return { id: crypto.randomUUID(), name: file.name, mimeType, dataUrl };
}
