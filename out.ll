@formatString = private constant [6 x i8] c"%lld\0A\00"
declare i32 @printf(ptr, ...)

define i32 @main() {
bb_0:
  %v0_0 = add i64 0, 1
  %v1_0 = add i64 0, %v0_0
  %v2_0 = add i64 0, 0
  %v3_0 = add i64 0, %v2_0
  %v4_0 = add i64 0, %v1_0
  %br_cond_4_0_0 = icmp ne i64 %v4_0, 0
  br i1 %br_cond_4_0_0, label %bb_1, label %bb_5
bb_1:
  %v5_0 = add i64 0, 1
  %fmt_ptr_5_0_1 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_5_0_1 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_5_0_1, i64 %v5_0)
  %v6_0 = add i64 0, 0
  %v7_0 = add i64 0, %v3_0
  %br_cond_7_0_1 = icmp ne i64 %v7_0, 0
  br i1 %br_cond_7_0_1, label %bb_2, label %bb_3
bb_2:
  %v8_0 = add i64 0, 2
  %fmt_ptr_8_0_2 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_8_0_2 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_8_0_2, i64 %v8_0)
  %v9_0 = add i64 0, 0
  br label %bb_4
bb_3:
  %v10_0 = add i64 0, 3
  %fmt_ptr_10_0_3 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_10_0_3 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_10_0_3, i64 %v10_0)
  %v11_0 = add i64 0, 0
  br label %bb_4
bb_4:
  br label %bb_9
bb_5:
  %v12_0 = add i64 0, 4
  %fmt_ptr_12_0_5 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_12_0_5 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_12_0_5, i64 %v12_0)
  %v13_0 = add i64 0, 0
  %v14_0 = add i64 0, %v3_0
  %br_cond_14_0_5 = icmp ne i64 %v14_0, 0
  br i1 %br_cond_14_0_5, label %bb_6, label %bb_7
bb_6:
  %v15_0 = add i64 0, 5
  %fmt_ptr_15_0_6 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_15_0_6 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_15_0_6, i64 %v15_0)
  %v16_0 = add i64 0, 0
  br label %bb_8
bb_7:
  %v17_0 = add i64 0, 6
  %fmt_ptr_17_0_7 = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0
  %print_ret_17_0_7 = call i32 (ptr, ...) @printf(ptr %fmt_ptr_17_0_7, i64 %v17_0)
  %v18_0 = add i64 0, 0
  br label %bb_8
bb_8:
  br label %bb_9
bb_9:
  %v19_0 = add i64 0, 0
  %ret32_19_0_9 = trunc i64 %v19_0 to i32
  ret i32 %ret32_19_0_9
}
