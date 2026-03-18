@formatString = private constant [6 x i8] c"%lld\0A\00"
declare i32 @printf(ptr, ...)

define i32 @main() {
bb_0:
  %v0_0 = add i64 0, 1
  %v1_0 = add i64 0, 0
  %v2_0 = add i64 0, 1
  %v3_0 = add i64 0, 10
  %v4_0 = add i64 0, 200
  %v5_0 = add i64 0, 50
  %v6_0 = add i64 0, 0
  %v7_0 = add i64 0, 1
  br label %bb_1
bb_1:
  %v0_1 = phi i64 [ %v0_0, %bb_0 ], [ %v0_2, %bb_2 ]
  %v4_1 = phi i64 [ %v4_0, %bb_0 ], [ %v4_2, %bb_2 ]
  %v1_1 = phi i64 [ %v1_0, %bb_0 ], [ %v1_4, %bb_2 ]
  %v3_1 = phi i64 [ %v3_0, %bb_0 ], [ %v3_3, %bb_2 ]
  %v2_1 = phi i64 [ %v2_0, %bb_0 ], [ %v2_4, %bb_2 ]
  %v5_1 = phi i64 [ %v5_0, %bb_0 ], [ %v5_2, %bb_2 ]
  %v8_0 = add i64 0, 20
  %cmp_9_0_1 = icmp sgt i64 %v0_1, %v8_0
  %v9_0 = zext i1 %cmp_9_0_1 to i64
  %br_cond_9_0_1 = icmp ne i64 %v9_0, 0
  br i1 %br_cond_9_0_1, label %bb_3, label %bb_2
bb_2:
  %v10_0 = add i64 0, 2
  %v11_0 = add i64 0, 3
  %v12_0 = add i64 0, 5
  %v13_0 = add i64 0, 10
  %v14_0 = add i64 0, 1
  %v15_1 = mul i64 %v0_1, %v10_0
  %v16_1 = add i64 %v1_1, %v15_1
  %v1_4 = add i64 %v16_1, %v11_0
  %v17_0 = add i64 %v0_1, %v14_0
  %v2_4 = mul i64 %v2_1, %v17_0
  %v18_0 = sdiv i64 %v2_4, %v12_0
  %v3_2 = add i64 %v3_1, %v18_0
  %v3_3 = sub i64 %v3_2, %v13_0
  %v4_2 = sub i64 %v4_1, %v0_1
  %v5_2 = add i64 %v5_1, %v10_0
  %v0_2 = add i64 %v0_1, %v14_0
  br label %bb_1
bb_3:
  %v19_0 = add i64 0, 0
  %v20_0 = add i64 0, 30
  %v21_0 = add i64 0, 7
  %v22_0 = add i64 0, 3
  br label %bb_4
bb_4:
  %v19_1 = phi i64 [ %v19_2, %bb_5 ], [ %v19_0, %bb_3 ]
  %v1_2 = phi i64 [ %v1_3, %bb_5 ], [ %v1_1, %bb_3 ]
  %v20_1 = phi i64 [ %v20_2, %bb_5 ], [ %v20_0, %bb_3 ]
  %v21_1 = phi i64 [ %v21_2, %bb_5 ], [ %v21_0, %bb_3 ]
  %v2_2 = phi i64 [ %v2_3, %bb_5 ], [ %v2_1, %bb_3 ]
  %v6_1 = phi i64 [ %v6_2, %bb_5 ], [ %v6_0, %bb_3 ]
  %v22_1 = phi i64 [ %v22_2, %bb_5 ], [ %v22_0, %bb_3 ]
  %v23_0 = add i64 0, 40
  %cmp_24_0_4 = icmp sge i64 %v19_1, %v23_0
  %v24_0 = zext i1 %cmp_24_0_4 to i64
  %br_cond_24_0_4 = icmp ne i64 %v24_0, 0
  br i1 %br_cond_24_0_4, label %bb_6, label %bb_5
bb_5:
  %v25_0 = add i64 0, 1
  %v26_0 = add i64 0, 2
  %v27_0 = add i64 0, 4
  %v28_0 = add i64 0, 6
  %v29_0 = add i64 0, 9
  %v30_0 = mul i64 %v19_1, %v26_0
  %v31_0 = add i64 %v20_1, %v30_0
  %v32_0 = sub i64 %v31_0, %v21_1
  %v33_0 = sdiv i64 %v32_0, %v26_0
  %v20_2 = add i64 %v33_0, %v22_1
  %v34_0 = mul i64 %v21_1, %v27_0
  %v35_0 = add i64 %v34_0, %v19_1
  %v36_0 = sdiv i64 %v35_0, %v26_0
  %v21_2 = add i64 %v36_0, %v25_0
  %v37_0 = add i64 %v22_1, %v28_0
  %v38_0 = sdiv i64 %v37_0, %v26_0
  %v22_2 = add i64 %v38_0, %v25_0
  %cmp_39_0_5 = icmp slt i64 %v20_2, %v21_2
  %v39_0 = zext i1 %cmp_39_0_5 to i64
  %cmp_40_0_5 = icmp sgt i64 %v22_2, %v29_0
  %v40_0 = zext i1 %cmp_40_0_5 to i64
  %v41_0 = and i64 %v39_0, %v40_0
  %not_tmp_42_0_5 = icmp eq i64 %v41_0, 0
  %v42_0 = zext i1 %not_tmp_42_0_5 to i64
  %v7_1 = or i64 %v7_0, %v42_0
  %v43_0 = add i64 %v6_1, %v20_2
  %v44_0 = sub i64 %v43_0, %v21_2
  %v6_2 = add i64 %v44_0, %v22_2
  %v1_3 = add i64 %v1_2, %v19_1
  %v2_3 = add i64 %v2_2, %v20_2
  %v19_2 = add i64 %v19_1, %v25_0
  br label %bb_4
bb_6:
  %v45_0 = add i64 0, 1
  %v46_0 = add i64 0, 0
  br label %bb_7
bb_7:
  %v46_1 = phi i64 [ %v46_0, %bb_6 ], [ %v46_2, %bb_11 ]
  %v45_1 = phi i64 [ %v45_0, %bb_6 ], [ %v45_2, %bb_11 ]
  %v47_0 = add i64 0, 8
  %cmp_48_0_7 = icmp sgt i64 %v45_1, %v47_0
  %v48_0 = zext i1 %cmp_48_0_7 to i64
  %br_cond_48_0_7 = icmp ne i64 %v48_0, 0
  br i1 %br_cond_48_0_7, label %bb_12, label %bb_8
bb_8:
  %v49_0 = add i64 0, 1
  %v50_0 = add i64 0, 0
  br label %bb_9
bb_9:
  %v50_1 = phi i64 [ %v50_2, %bb_10 ], [ %v50_0, %bb_8 ]
  %v49_1 = phi i64 [ %v49_2, %bb_10 ], [ %v49_0, %bb_8 ]
  %v51_0 = add i64 0, 12
  %cmp_52_0_9 = icmp sgt i64 %v49_1, %v51_0
  %v52_0 = zext i1 %cmp_52_0_9 to i64
  %br_cond_52_0_9 = icmp ne i64 %v52_0, 0
  br i1 %br_cond_52_0_9, label %bb_11, label %bb_10
bb_10:
  %v53_0 = add i64 0, 1
  %v54_0 = add i64 0, 2
  %v55_0 = add i64 0, 3
  %v56_0 = mul i64 %v45_1, %v49_1
  %v57_0 = add i64 %v56_0, %v55_0
  %v58_0 = mul i64 %v57_0, %v54_0
  %v59_0 = sdiv i64 %v58_0, %v54_0
  %v60_0 = sub i64 %v59_0, %v49_1
  %v61_0 = add i64 %v60_0, %v45_1
  %v50_2 = add i64 %v50_1, %v61_0
  %v49_2 = add i64 %v49_1, %v53_0
  br label %bb_9
bb_11:
  %v46_2 = add i64 %v46_1, %v50_1
  %v62_0 = add i64 0, 1
  %v45_2 = add i64 %v45_1, %v62_0
  br label %bb_7
bb_12:
  %v63_0 = add i64 0, 100
  %v64_0 = add i64 0, 1
  %v65_0 = add i64 0, 0
  br label %bb_13
bb_13:
  %v65_1 = phi i64 [ %v65_0, %bb_12 ], [ %v65_2, %bb_14 ]
  %v64_1 = phi i64 [ %v64_0, %bb_12 ], [ %v64_2, %bb_14 ]
  %v63_1 = phi i64 [ %v63_0, %bb_12 ], [ %v63_2, %bb_14 ]
  %v66_0 = add i64 0, 0
  %cmp_67_0_13 = icmp sgt i64 %v63_1, %v66_0
  %v67_0 = zext i1 %cmp_67_0_13 to i64
  %br_cond_67_0_13 = icmp ne i64 %v67_0, 0
  br i1 %br_cond_67_0_13, label %bb_14, label %bb_15
bb_14:
  %v68_0 = add i64 0, 1
  %v69_0 = add i64 0, 2
  %v70_0 = add i64 0, 5
  %v71_0 = add i64 0, 10
  %v72_0 = mul i64 %v64_1, %v69_0
  %v73_0 = add i64 %v72_0, %v63_1
  %v74_0 = sdiv i64 %v73_0, %v70_0
  %v64_2 = add i64 %v74_0, %v68_0
  %v15_0 = add i64 %v65_1, %v64_2
  %v16_0 = sub i64 %v15_0, %v71_0
  %v65_2 = add i64 %v16_0, %v63_1
  %v63_2 = sub i64 %v63_1, %v68_0
  br label %bb_13
bb_15:
  %v75_0 = add i64 %v1_2, %v2_2
  %v76_0 = add i64 %v75_0, %v3_1
  %v77_0 = add i64 %v76_0, %v4_1
  %v78_0 = add i64 %v77_0, %v5_1
  %v79_0 = add i64 %v78_0, %v6_1
  %v80_0 = add i64 %v79_0, %v46_1
  %v81_0 = add i64 %v80_0, %v65_1
  %fmt_ptr_1_2_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_1_2_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_1_2_15, i64 %v1_2)
  %fmt_ptr_2_2_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_2_2_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_2_2_15, i64 %v2_2)
  %fmt_ptr_3_1_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_3_1_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_3_1_15, i64 %v3_1)
  %fmt_ptr_4_1_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_4_1_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_4_1_15, i64 %v4_1)
  %fmt_ptr_5_1_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_5_1_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_5_1_15, i64 %v5_1)
  %fmt_ptr_6_1_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_6_1_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_6_1_15, i64 %v6_1)
  %fmt_ptr_46_1_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_46_1_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_46_1_15, i64 %v46_1)
  %fmt_ptr_65_1_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_65_1_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_65_1_15, i64 %v65_1)
  %fmt_ptr_81_0_15 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_81_0_15 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_81_0_15, i64 %v81_0)
  %v82_0 = add i64 0, 0
  %ret32_82_0_15 = trunc i64 %v82_0 to i32
  ret i32 %ret32_82_0_15
}
