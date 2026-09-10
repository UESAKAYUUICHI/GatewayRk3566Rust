import { createApp } from 'vue'
import { createPinia } from 'pinia'
import './style.css'
import App from './App.vue'
import router from './router'

const isEditableTarget=(target:EventTarget|null)=>{
  if(!(target instanceof HTMLElement))return false
  const tag=target.tagName.toLowerCase()
  return target.isContentEditable||tag==='input'||tag==='textarea'||tag==='select'
}

const installKioskGuards=()=>{
  window.addEventListener('contextmenu',event=>event.preventDefault())
  window.addEventListener('dragstart',event=>event.preventDefault())
  window.addEventListener('keydown',event=>{
    const key=event.key.toLowerCase()
    const blocksBrowserShortcut=
      event.altKey||
      key==='browserback'||
      key==='browserforward'||
      key==='f5'||
      key==='f11'||
      (key==='backspace'&&!isEditableTarget(event.target))||
      ((event.ctrlKey||event.metaKey)&&['l','r','w','n','t','p','o','s','h','j'].includes(key))
    if(blocksBrowserShortcut){
      event.preventDefault()
      event.stopPropagation()
    }
  },true)

  let startX=0,startY=0
  window.addEventListener('touchstart',event=>{
    const touch=event.touches[0]
    if(!touch)return
    startX=touch.clientX
    startY=touch.clientY
  },{passive:true})
  window.addEventListener('touchmove',event=>{
    const touch=event.touches[0]
    if(!touch)return
    const dx=touch.clientX-startX
    const dy=touch.clientY-startY
    if(Math.abs(dx)>60&&Math.abs(dx)>Math.abs(dy)*1.15){
      event.preventDefault()
      event.stopPropagation()
    }
  },{passive:false,capture:true})
}

installKioskGuards()
createApp(App).use(createPinia()).use(router).mount('#app')
