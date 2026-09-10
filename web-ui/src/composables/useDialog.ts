import { reactive } from 'vue'
const state=reactive({open:false,title:'操作提示',message:'',danger:false})
export function useDialog(){
  const show=(title:string,message:string,danger=false)=>Object.assign(state,{open:true,title,message,danger})
  const close=()=>{state.open=false;state.danger=false}
  return{state,show,close}
}
