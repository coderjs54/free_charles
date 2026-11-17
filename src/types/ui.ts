export interface TabItem {
  key: string
  label: string
  badge?: string | number
}

export interface ContextMenuItem {
  id: string
  label: string
  icon?: string
  shortcut?: string
  danger?: boolean
  disabled?: boolean
  separator?: boolean
}
