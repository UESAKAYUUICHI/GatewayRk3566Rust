<script setup lang="ts">withDefaults(defineProps<{ok?:boolean;text:string}>(),{ok:true})</script>
<template><span class="status-badge" :class="ok?'ok':'bad'"><i/>{{text}}</span></template>
