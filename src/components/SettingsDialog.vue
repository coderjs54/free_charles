<script setup lang="ts">
import { api } from '../lib/api'
import { computed, onMounted, reactive, ref } from 'vue'
import type { HttpsCaInfo } from '../types/models'
import { useSession } from '../stores/useSession'
import { useToast } from '../stores/useToast'
import { errorMessage } from '../utils/error'
import AppIcon from './AppIcon.vue'
import AppModal from './AppModal.vue'

const emit = defineEmits<{ (event: 'close'): void }>()

const { settings, proxyAddress, proxyStatus, systemProxyState, updateSettings } = useSession()
const { push } = useToast()

const form = reactive({
  proxyPort: settings.value?.proxyPort ?? 8888,
  autoStartProxy: settings.value?.autoStartProxy ?? true,
  allowLan: settings.value?.allowLan ?? false,
  autoSystemProxy: settings.value?.autoSystemProxy ?? true,
  decryptHttps: settings.value?.decryptHttps ?? false,
  allowInsecureUpstream: settings.value?.allowInsecureUpstream ?? false,
  recordTunnels: settings.value?.recordTunnels ?? true,
  bodyLimitMb: Math.round(((settings.value?.maxBodySize ?? 5 * 1024 * 1024) / 1024 / 1024) * 10) / 10,
  maxEntries: settings.value?.maxEntries ?? 3000
})
const errorText = ref('')
const saving = ref(false)

// 本地 HTTPS 解密根证书状态
const caInfo = ref<HttpsCaInfo | null>(null)
const caBusy = ref(false)

const caStatusText = computed(() => {
  const info = caInfo.value
  if (!info) return '读取根证书状态…'
  if (info.error) return info.error
  if (!info.generated) return '尚未生成根证书'
  if (info.installable) return '已生成，可安装到当前用户 Root 证书存储'
  return '已生成，请手动安装到浏览器或系统信任存储'
})

const caFileName = computed(() => {
  const path = caInfo.value?.certPath ?? ''
  return path.split(/[\\/]/).pop() ?? ''
})

const systemProxyStatusText = computed(() => {
  const state = systemProxyState.value
  if (!state.supported) return '当前系统不支持自动配置，请在浏览器或系统中手动设置 HTTP 代理'
  if (state.error) return state.error
  if (state.active) return `已接管系统代理：${state.server}`
  if (form.autoSystemProxy) return '代理启动后会自动接管系统代理'
  return '未启用自动系统代理，需要手动配置浏览器代理'
})

async function copyAddress(): Promise<void> {
  try {
    await api.copyText(proxyAddress.value)
    push(`已复制代理地址 ${proxyAddress.value}`)
  } catch {
    push('复制失败', 'error')
  }
}

async function refreshCaInfo(): Promise<void> {
  try {
    caInfo.value = await api.getHttpsCaInfo()
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

async function generateCa(): Promise<void> {
  if (caBusy.value) return
  caBusy.value = true
  try {
    const info = await api.ensureHttpsCa()
    caInfo.value = info
    if (info.error) push(info.error, 'error')
    else push('HTTPS 解密根证书已生成，请继续安装并信任', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  } finally {
    caBusy.value = false
  }
}

async function installCa(): Promise<void> {
  if (caBusy.value) return
  caBusy.value = true
  try {
    const info = await api.installHttpsCa()
    caInfo.value = info
    if (info.error) push(info.error, 'error')
    else push(info.message ?? '根证书安装完成', 'success')
  } catch (error) {
    push(errorMessage(error), 'error')
  } finally {
    caBusy.value = false
  }
}

async function revealCa(): Promise<void> {
  try {
    caInfo.value = await api.revealHttpsCa()
  } catch (error) {
    push(errorMessage(error), 'error')
  }
}

onMounted(() => {
  void refreshCaInfo()
})

async function save(): Promise<void> {
  errorText.value = ''
  if (form.decryptHttps && !caInfo.value?.generated) {
    errorText.value = '请先生成 HTTPS 解密根证书，并安装到系统信任存储后再开启解密'
    return
  }
  const port = Number(form.proxyPort)
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    errorText.value = '代理端口必须是 1024 - 65535 之间的整数'
    return
  }
  const bodyLimitMb = Number(form.bodyLimitMb)
  if (!Number.isFinite(bodyLimitMb) || bodyLimitMb < 0.0625 || bodyLimitMb > 50) {
    errorText.value = 'Body 上限需在 0.0625 MB - 50 MB 之间'
    return
  }
  const maxEntries = Number(form.maxEntries)
  if (!Number.isInteger(maxEntries) || maxEntries < 100 || maxEntries > 20000) {
    errorText.value = '会话条目数需在 100 - 20000 之间'
    return
  }

  saving.value = true
  try {
    await updateSettings({
      proxyPort: port,
      autoStartProxy: form.autoStartProxy,
      allowLan: form.allowLan,
      autoSystemProxy: form.autoSystemProxy,
      decryptHttps: form.decryptHttps,
      allowInsecureUpstream: form.allowInsecureUpstream,
      recordTunnels: form.recordTunnels,
      maxBodySize: Math.round(bodyLimitMb * 1024 * 1024),
      maxEntries
    })
    if (proxyStatus.value.error) {
      errorText.value = proxyStatus.value.error
      push(proxyStatus.value.error, 'error')
      return
    }
    push('设置已保存', 'success')
    emit('close')
  } catch (error) {
    errorText.value = errorMessage(error)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <AppModal title="代理设置" width="580px" @close="emit('close')">
    <div class="fc-settings">
      <section class="fc-settings-section">
        <h3 class="fc-settings-heading">代理服务</h3>

        <div class="fc-settings-row">
          <label class="fc-field">
            <span class="fc-field-label">代理端口</span>
            <input v-model.number="form.proxyPort" class="fc-input fc-input--port" type="number" min="1024" max="65535" />
            <span class="fc-field-hint">默认 8888，修改后会自动重启代理</span>
          </label>
          <div class="fc-address-preview">
            <span class="fc-field-label">当前代理地址</span>
            <div class="fc-address-value">
              <code>{{ proxyAddress }}</code>
              <button class="fc-address-copy" type="button" title="复制代理地址" @click="copyAddress">
                <AppIcon name="copy" :size="12" />
              </button>
            </div>
          </div>
        </div>

        <label class="fc-switch-row">
          <span class="fc-switch-text">
            <span class="fc-switch-title">允许局域网设备连接</span>
            <span class="fc-switch-desc">开启后绑定 0.0.0.0，手机或其他设备可通过本机 IP 连接代理</span>
          </span>
          <span class="fc-switch">
            <input v-model="form.allowLan" class="fc-switch-input" type="checkbox" />
            <span class="fc-switch-track"><span class="fc-switch-thumb" /></span>
          </span>
        </label>

        <label class="fc-switch-row">
          <span class="fc-switch-text">
            <span class="fc-switch-title">启动时自动开启代理</span>
            <span class="fc-switch-desc">应用启动后立即监听代理端口</span>
          </span>
          <span class="fc-switch">
            <input v-model="form.autoStartProxy" class="fc-switch-input" type="checkbox" />
            <span class="fc-switch-track"><span class="fc-switch-thumb" /></span>
          </span>
        </label>

        <label class="fc-switch-row">
          <span class="fc-switch-text">
            <span class="fc-switch-title">自动设置 Windows 系统代理</span>
            <span class="fc-switch-desc">
              启动代理时自动接管系统代理，停止代理或退出应用时恢复原设置；浏览器和其他遵循系统代理的程序无需手动配置（仅 Windows，首次生效前会请求确认）
            </span>
          </span>
          <span class="fc-switch">
            <input v-model="form.autoSystemProxy" class="fc-switch-input" type="checkbox" />
            <span class="fc-switch-track"><span class="fc-switch-thumb" /></span>
          </span>
        </label>

        <div
          class="fc-system-proxy-status"
          :class="{
            'fc-system-proxy-status--active': systemProxyState.active,
            'fc-system-proxy-status--error': Boolean(systemProxyState.error),
            'fc-system-proxy-status--disabled': !form.autoSystemProxy
          }"
        >
          <span class="fc-system-proxy-dot" />
          <span class="fc-system-proxy-text">{{ systemProxyStatusText }}</span>
        </div>
      </section>

      <section class="fc-settings-section">
        <h3 class="fc-settings-heading">捕获行为</h3>

        <label class="fc-switch-row">
          <span class="fc-switch-text">
            <span class="fc-switch-title">解密 HTTPS 内容</span>
            <span class="fc-switch-desc">
              使用本地根证书对 HTTPS 做 TLS 中间人解密。必须先在系统中安装并信任根证书；仅用于你拥有或获授权的设备和流量。
            </span>
          </span>
          <span class="fc-switch">
            <input
              v-model="form.decryptHttps"
              class="fc-switch-input"
              type="checkbox"
              data-testid="decrypt-https"
            />
            <span class="fc-switch-track"><span class="fc-switch-thumb" /></span>
          </span>
        </label>

        <div class="fc-ca-card">
          <div class="fc-ca-head">
            <span class="fc-ca-title">HTTPS 解密根证书</span>
            <span
              class="fc-ca-state"
              :class="{
                'fc-ca-state--ready': caInfo?.generated,
                'fc-ca-state--error': Boolean(caInfo?.error)
              }"
            >
              {{ caStatusText }}
            </span>
          </div>
          <p class="fc-ca-desc">
            根证书只保存在本机。安装根证书后，代理可以解密经过它的 HTTPS 流量；抓包结束后建议在系统证书管理器中移除信任。
          </p>
          <div class="fc-ca-actions">
            <button
              v-if="!caInfo?.generated"
              class="fc-ca-button"
              type="button"
              data-testid="https-ca-generate"
              :disabled="caBusy"
              @click="generateCa"
            >
              {{ caBusy ? '生成中…' : '生成根证书' }}
            </button>
            <button
              v-else-if="caInfo?.installable"
              class="fc-ca-button fc-ca-button--primary"
              type="button"
              data-testid="https-ca-install"
              :disabled="caBusy"
              @click="installCa"
            >
              {{ caBusy ? '安装中…' : '安装/信任根证书' }}
            </button>
            <button v-if="caInfo?.generated" class="fc-ca-button" type="button" @click="revealCa">
              打开证书位置
            </button>
          </div>
          <p v-if="caFileName" class="fc-ca-path" :title="caInfo?.certPath">
            证书文件：{{ caFileName }}
          </p>
        </div>

        <label class="fc-switch-row">
          <span class="fc-switch-text">
            <span class="fc-switch-title">忽略上游 HTTPS 证书错误</span>
            <span class="fc-switch-desc">
              仅用于自签名或内部证书的上游服务器。开启后会跳过代理到目标服务器的证书校验，存在安全风险；不影响浏览器对本地根证书的信任。
            </span>
          </span>
          <span class="fc-switch">
            <input
              v-model="form.allowInsecureUpstream"
              class="fc-switch-input"
              type="checkbox"
              data-testid="allow-insecure-upstream"
            />
            <span class="fc-switch-track"><span class="fc-switch-thumb" /></span>
          </span>
        </label>

        <label class="fc-switch-row">
          <span class="fc-switch-text">
            <span class="fc-switch-title">记录未解密的 HTTPS 隧道</span>
            <span class="fc-switch-desc">
              关闭 HTTPS 解密时，为 CONNECT 连接生成提示记录；解密成功的请求不受此开关影响
            </span>
          </span>
          <span class="fc-switch">
            <input v-model="form.recordTunnels" class="fc-switch-input" type="checkbox" />
            <span class="fc-switch-track"><span class="fc-switch-thumb" /></span>
          </span>
        </label>

        <div class="fc-settings-grid">
          <label class="fc-field">
            <span class="fc-field-label">单个 Body 上限（MB）</span>
            <input v-model.number="form.bodyLimitMb" class="fc-input" type="number" min="0.0625" max="50" step="0.5" />
            <span class="fc-field-hint">超出部分会被截断，默认 5 MB</span>
          </label>
          <label class="fc-field">
            <span class="fc-field-label">会话最大条目数</span>
            <input v-model.number="form.maxEntries" class="fc-input" type="number" min="100" max="20000" step="100" />
            <span class="fc-field-hint">超出后自动丢弃最早的请求</span>
          </label>
        </div>
      </section>

      <div class="fc-settings-tip">
        <AppIcon name="info" :size="14" />
        <span>
          在浏览器 / 系统中把 <strong>HTTP 代理</strong> 指向
          <code>{{ proxyAddress }}</code>
          即可抓取 HTTP 请求。HTTPS 默认以隧道形式透传；需要查看内容时，请开启上方 HTTPS 解密并信任本地根证书。
        </span>
      </div>

      <div v-if="errorText" class="fc-settings-error">{{ errorText }}</div>
    </div>

    <template #footer>
      <button class="fc-button" type="button" :disabled="saving" @click="emit('close')">取消</button>
      <button class="fc-button fc-button--primary" type="button" :disabled="saving" @click="save">
        {{ saving ? '保存中…' : '保存并应用' }}
      </button>
    </template>
  </AppModal>
</template>

<style scoped lang="scss">
.fc-settings {
  display: flex;
  flex-direction: column;
  gap: $spacing-lg;
}

.fc-settings-section {
  padding-bottom: $spacing-md;
  border-bottom: 1px solid $color-divider;
}

.fc-settings-section:last-of-type {
  border-bottom: 0;
  padding-bottom: 0;
}

.fc-settings-heading {
  margin-bottom: $spacing-md;
  color: $color-text-tertiary;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.07em;
  text-transform: uppercase;
}

.fc-settings-row {
  display: flex;
  gap: $spacing-xl;
  align-items: flex-start;
}

.fc-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.fc-field-label {
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 500;
}

.fc-field-hint {
  color: $color-text-tertiary;
  font-size: 10.5px;
  line-height: 1.5;
}

.fc-input {
  width: 100%;
  height: 32px;
  padding: 0 9px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text;
  font-family: $font-mono;
  font-size: 13px;
  transition: border-color $transition-fast, box-shadow $transition-fast;
}

.fc-input:focus {
  border-color: rgba(37, 99, 235, 0.6);
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}

.fc-input--port {
  max-width: 160px;
}

.fc-address-preview {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.fc-address-value {
  display: flex;
  align-items: center;
  gap: $spacing-sm;
  height: 32px;
  padding: 0 6px 0 9px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface-muted;
}

.fc-address-value code {
  flex: 1;
  overflow: hidden;
  color: $color-text-secondary;
  font-family: $font-mono;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-address-copy {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: $radius-xs;
  color: $color-text-tertiary;
}

.fc-address-copy:hover {
  background: $color-hover;
  color: $color-text;
}

.fc-switch-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: $spacing-xl;
  padding: 8px 0;
  cursor: pointer;
}

.fc-switch-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.fc-switch-title {
  color: $color-text;
  font-size: 13px;
  font-weight: 500;
}

.fc-switch-desc {
  color: $color-text-tertiary;
  font-size: 11px;
  line-height: 1.5;
}

.fc-switch {
  position: relative;
  display: inline-flex;
  flex: none;
}

.fc-switch-input {
  position: absolute;
  width: 1px;
  height: 1px;
  opacity: 0;
}

.fc-switch-track {
  position: relative;
  display: block;
  width: 32px;
  height: 18px;
  border-radius: $radius-pill;
  background: $color-switch-track;
  transition: background $transition-fast;
}

.fc-switch-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: $color-switch-thumb;
  box-shadow: 0 1px 2px rgba(16, 24, 40, 0.2);
  transition: transform $transition-fast;
}

.fc-switch-input:checked + .fc-switch-track {
  background: $color-primary;
}

.fc-switch-input:checked + .fc-switch-track .fc-switch-thumb {
  transform: translateX(14px);
}

.fc-switch-input:focus-visible + .fc-switch-track {
  box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.18);
}

.fc-settings-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: $spacing-lg;
  margin-top: $spacing-sm;
}

.fc-system-proxy-status {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 2px;
  padding: 6px 9px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface-muted;
  color: $color-text-tertiary;
  font-size: 11px;
  line-height: 1.5;
}

.fc-system-proxy-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: $color-dot-idle;
  flex: none;
}

.fc-system-proxy-status--active {
  border-color: rgba(17, 126, 90, 0.2);
  background: $color-success-soft;
  color: $color-success;
}

.fc-system-proxy-status--active .fc-system-proxy-dot {
  background: $color-success;
  box-shadow: 0 0 0 2px rgba(17, 126, 90, 0.12);
}

.fc-system-proxy-status--error {
  border-color: rgba(192, 43, 43, 0.2);
  background: $color-danger-soft;
  color: $color-danger;
}

.fc-system-proxy-status--error .fc-system-proxy-dot {
  background: $color-danger;
}

.fc-system-proxy-status--disabled {
  opacity: 0.8;
}

.fc-system-proxy-text {
  overflow-wrap: anywhere;
}

.fc-ca-card {
  margin: 2px 0 $spacing-md;
  padding: 11px 13px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface-muted;
}

.fc-ca-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: $spacing-md;
}

.fc-ca-title {
  color: $color-text-secondary;
  font-size: 12px;
  font-weight: 600;
}

.fc-ca-state {
  overflow: hidden;
  color: $color-text-tertiary;
  font-size: 10.5px;
  text-align: right;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-ca-state--ready {
  color: $color-success;
}

.fc-ca-state--error {
  color: $color-danger;
}

.fc-ca-desc {
  margin-top: 6px;
  color: $color-text-tertiary;
  font-size: 10.5px;
  line-height: 1.6;
}

.fc-ca-actions {
  display: flex;
  flex-wrap: wrap;
  gap: $spacing-sm;
  margin-top: 8px;
}

.fc-ca-button {
  height: 26px;
  padding: 0 10px;
  border: 1px solid $color-border;
  border-radius: $radius-xs;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 11px;
}

.fc-ca-button:hover {
  border-color: $color-border-strong;
  background: $color-surface;
  color: $color-text;
}

.fc-ca-button--primary {
  border-color: $color-primary;
  background: $color-primary;
  color: #fff;
}

.fc-ca-button--primary:hover {
  border-color: $color-primary-hover;
  background: $color-primary-hover;
  color: #fff;
}

.fc-ca-button:disabled {
  opacity: 0.55;
  cursor: default;
}

.fc-ca-path {
  margin-top: 7px;
  overflow: hidden;
  color: $color-text-tertiary;
  font-family: $font-mono;
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.fc-settings-tip {
  display: flex;
  align-items: flex-start;
  gap: $spacing-md;
  padding: 9px 11px;
  border: 1px solid rgba(37, 99, 235, 0.14);
  border-radius: $radius-sm;
  background: $color-primary-soft;
  color: $color-text-secondary;
  font-size: 11.5px;
  line-height: 1.65;
}

.fc-settings-tip code {
  padding: 1px 4px;
  border-radius: 3px;
  background: rgba(37, 99, 235, 0.08);
  color: $color-primary;
  font-family: $font-mono;
  font-size: 10.5px;
}

.fc-settings-tip :deep(.fc-icon) {
  margin-top: 2px;
  color: $color-primary;
}

.fc-settings-error {
  padding: 8px 10px;
  border: 1px solid rgba(192, 43, 43, 0.2);
  border-radius: $radius-sm;
  background: $color-danger-soft;
  color: $color-danger;
  font-size: 12px;
  line-height: 1.5;
}

.fc-button {
  height: 30px;
  padding: 0 14px;
  border: 1px solid $color-border;
  border-radius: $radius-sm;
  background: $color-surface;
  color: $color-text-secondary;
  font-size: 12.5px;
  font-weight: 500;
  transition: border-color $transition-fast, background $transition-fast;
}

.fc-button:hover {
  border-color: $color-border-strong;
  background: $color-surface-muted;
}

.fc-button--primary {
  border-color: $color-primary;
  background: $color-primary;
  color: #fff;
}

.fc-button--primary:hover {
  border-color: $color-primary-hover;
  background: $color-primary-hover;
}

.fc-button:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
